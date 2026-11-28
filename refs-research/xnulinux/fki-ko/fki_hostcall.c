// SPDX-License-Identifier: GPL-2.0-only
/*
 * Host calls: a message channel from the foreign kernel to a Linux user process (a host service such as the DRM bridge,
 * gpu/xnu-drm/xnudrmd). The foreign kernel calls fki_hostcall() with a request; the service reads it from
 * /dev/xnu-hostcall, does the work with ordinary system calls and writes the reply back. fki only moves bytes: the
 * message format belongs to the two ends.
 *
 * One call at a time. The calling foreign CPU is blocked in the call and deaf to the other foreign CPUs' interrupts, so
 * a call with no service to answer it fails at once (-ENODEV), not after HC_TIMEOUT: the other CPUs would time out
 * waiting on that one (an XNU panic). The service also asks fki to translate its own mappings into physical addresses
 * (XNU_HOSTCALL_XLATE), so the foreign kernel can map the same pages. Pages nothing else holds in place (shared memory,
 * not a driver's buffer) it pins first (XNU_HOSTCALL_PIN), until XNU_HOSTCALL_UNPIN or its close; a device mapping
 * (VM_IO, VM_PFNMAP) has no pages to pin and is refused with -EOPNOTSUPP.
 */
#include "fki.h"
#include "fki_internal.h"
#include <linux/fs.h>
#include <linux/miscdevice.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/sched.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/version.h>
#include <linux/vmalloc.h>
#include <linux/wait.h>

#define HC_MAX ((8UL << 20) + 4096)	/* largest request or reply: the DRM bridge's 8 MiB of payload and its header */
#define HC_TIMEOUT (120 * HZ)		/* a service that does not answer: the call fails */

struct xnu_hostcall_xlate {
	__u64 va, len;			/* a range of the caller's address space, 4 KiB aligned */
	__u64 out;			/* user pointer: one __u64 physical address per 4 KiB page */
	__u64 max;			/* entries at out */
};
#define XNU_HOSTCALL_XLATE _IOWR('X', 1, struct xnu_hostcall_xlate)
struct xnu_hostcall_range {
	__u64 va, len;			/* 4 KiB aligned; UNPIN: the va of a PIN (len unused) */
};
#define XNU_HOSTCALL_PIN _IOW('X', 2, struct xnu_hostcall_range)
#define XNU_HOSTCALL_UNPIN _IOW('X', 3, struct xnu_hostcall_range)
#define HC_PIN_MAX ((1UL << 30) >> PAGE_SHIFT)

struct hc_pin { struct list_head link; unsigned long va, n; struct page **pages; };
struct hc_file { struct mutex lock; struct list_head pins; };

static DECLARE_WAIT_QUEUE_HEAD(hc_req_wait);
static DECLARE_WAIT_QUEUE_HEAD(hc_done_wait);
static DEFINE_SPINLOCK(hc_lock);
static void *hc_buf;			/* request, then reply */
static unsigned long hc_len;
static u64 hc_seq;			/* the call in flight (0: none) */
static int hc_state;			/* 0 idle, 1 request waiting, 2 read by the service, 3 replied */
static long hc_result;
static int hc_open;			/* the service's opens of /dev/xnu-hostcall (0: no service, calls fail at once) */
static int hc_copying;			/* a read() or write() is copying out of or into hc_buf: the call does not end under it */

struct hc_call { const void *req; unsigned long len; void *reply; unsigned long max; long result; };
static DEFINE_MUTEX(hc_mutex);		/* one call at a time through the single slot (several CPUs may call) */

static void hc_call_body(void *p)
{
	struct hc_call *c = p;
	static u64 seq;
	long r;

	local_irq_enable();
	preempt_enable();
	mutex_lock(&hc_mutex);
	if (!hc_buf)
		hc_buf = kvmalloc(HC_MAX, GFP_KERNEL);
	if (!hc_buf || c->len > HC_MAX) {
		r = hc_buf ? -EMSGSIZE : -ENOMEM;
		goto out;
	}
	preempt_disable(); local_irq_disable();
	memcpy(hc_buf, c->req, c->len);		/* the foreign kernel's memory: read with interrupts off */
	local_irq_enable(); preempt_enable();
	spin_lock(&hc_lock);
	if (!hc_open) {
		spin_unlock(&hc_lock);
		r = -ENODEV;
		goto out;
	}
	hc_len = c->len;
	hc_seq = ++seq;
	hc_state = 1;
	spin_unlock(&hc_lock);
	wake_up_interruptible(&hc_req_wait);
	/* until the reply, or the service closing its device with the call unanswered */
	wait_event_timeout(hc_done_wait, READ_ONCE(hc_state) == 3 || !READ_ONCE(hc_open), HC_TIMEOUT);
	spin_lock(&hc_lock);
	while (hc_copying) {	/* the next call would fill hc_buf under the service's copy, or its late reply would land on it */
		spin_unlock(&hc_lock);
		wait_event(hc_done_wait, !READ_ONCE(hc_copying));
		spin_lock(&hc_lock);
	}
	if (hc_state != 3) {
		r = hc_open ? -ETIMEDOUT : -ENODEV;
		hc_state = 0; hc_seq = 0;
		spin_unlock(&hc_lock);
		goto out;
	}
	spin_unlock(&hc_lock);
	r = hc_result;
	if (r >= 0) {
		unsigned long n = min_t(unsigned long, hc_len, c->max);
		preempt_disable(); local_irq_disable();
		memcpy(c->reply, hc_buf, n);
		local_irq_enable(); preempt_enable();
		r = hc_len > c->max ? -EMSGSIZE : (long)n;
	}
	spin_lock(&hc_lock);
	hc_state = 0; hc_seq = 0;
	spin_unlock(&hc_lock);
out:
	mutex_unlock(&hc_mutex);
	preempt_disable();
	local_irq_disable();
	c->result = r;
}

/*
 * From inside fki_enter(): send len bytes at req to the service and wait for its reply (at most max bytes into reply).
 * Returns the reply's length, -EMSGSIZE when it does not fit, -ETIMEDOUT when no service answered in time.
 */
long fki_hostcall(const void *req, unsigned long len, void *reply, unsigned long max)
{
	struct hc_call c = { req, len, reply, max, -EIO };

	fki_linux_runs_ext(hc_call_body, &c);
	return c.result;
}
EXPORT_SYMBOL(fki_hostcall);

/* The service: read() the next request, prefixed with its 8-byte sequence number. */
static ssize_t hc_read(struct file *f, char __user *buf, size_t n, loff_t *off)
{
	unsigned long len;
	u64 seq;
	int err;

	err = wait_event_interruptible(hc_req_wait, READ_ONCE(hc_state) == 1);
	if (err)
		return err;
	spin_lock(&hc_lock);
	if (hc_state != 1 || n < 8 + hc_len) {
		err = hc_state != 1 ? -EAGAIN : -EMSGSIZE;	/* too small a buffer: the request stays, for a read() with room */
		spin_unlock(&hc_lock);
		return err;
	}
	hc_state = 2;
	seq = hc_seq;
	len = hc_len;
	hc_copying = 1;
	spin_unlock(&hc_lock);
	if (copy_to_user(buf, &seq, 8) || copy_to_user(buf + 8, hc_buf, len))
		err = -EFAULT;
	spin_lock(&hc_lock);
	hc_copying = 0;
	if (err)
		hc_state = 1;		/* not delivered: the next read() gets it */
	spin_unlock(&hc_lock);
	wake_up(&hc_done_wait);
	if (err)
		wake_up_interruptible(&hc_req_wait);
	return err ? err : 8 + len;
}

/* The service: write() the reply, prefixed with the request's sequence number. */
static ssize_t hc_write(struct file *f, const char __user *buf, size_t n, loff_t *off)
{
	u64 seq;
	int err = 0;

	if (n < 8 || n - 8 > HC_MAX)
		return -EINVAL;
	if (copy_from_user(&seq, buf, 8))
		return -EFAULT;
	spin_lock(&hc_lock);
	if (hc_state != 2 || seq != hc_seq || hc_copying) {
		spin_unlock(&hc_lock);
		return -ESTALE;		/* the call timed out meanwhile */
	}
	hc_copying = 1;
	spin_unlock(&hc_lock);
	if (copy_from_user(hc_buf, buf + 8, n - 8))
		err = -EFAULT;		/* no reply yet: the service may write it again */
	spin_lock(&hc_lock);
	hc_copying = 0;
	if (!err) {
		hc_len = n - 8;
		hc_result = 0;
		hc_state = 3;
	}
	spin_unlock(&hc_lock);
	wake_up(&hc_done_wait);
	return err ? err : n;
}

/*
 * The page frame behind va in a device mapping (VM_IO, VM_PFNMAP), with mmap_lock held. The kernel's call for it changed
 * within the kernels of one Ubuntu release (24.04: 6.8, then 6.11, 6.14, 6.17 and 7.0 as HWE kernels): follow_pfn() up
 * to 6.11, follow_pfnmap_start() from 6.12 (6.14 has no follow_pfn() any more).
 */
static int hc_device_pfn(struct vm_area_struct *vma, unsigned long va, unsigned long *pfn)
{
#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 12, 0)
	struct follow_pfnmap_args args = { .vma = vma, .address = va };
	int err = follow_pfnmap_start(&args);

	if (err)
		return err;
	*pfn = args.pfn;
	follow_pfnmap_end(&args);
	return 0;
#else
	return follow_pfn(vma, va, pfn);
#endif
}

static long hc_xlate_one(struct mm_struct *mm, unsigned long va, u64 *pa)
{
	struct vm_area_struct *vma = vma_lookup(mm, va);
	struct page *pg;

	if (!vma)
		return -EFAULT;
	if (vma->vm_flags & (VM_IO | VM_PFNMAP)) {
		unsigned long pfn;
		int err = hc_device_pfn(vma, va, &pfn);

		if (err)
			return err;
		*pa = (u64)pfn << PAGE_SHIFT;
		return 0;
	}
	if (get_user_pages_remote(mm, va, 1, FOLL_WRITE, &pg, NULL) != 1)
		return -EFAULT;
	*pa = page_to_phys(pg);
	put_page(pg);		/* the mapping's owner (a GEM object) keeps the page */
	return 0;
}

static long hc_xlate(struct xnu_hostcall_xlate __user *ua)
{
	struct xnu_hostcall_xlate a;
	struct mm_struct *mm = current->mm;
	unsigned long i, k, n;
	u64 pa[64];
	long err = 0;

	if (copy_from_user(&a, ua, sizeof(a)))
		return -EFAULT;
	if ((a.va | a.len) & (PAGE_SIZE - 1))
		return -EINVAL;
	n = a.len >> PAGE_SHIFT;
	if (n > a.max)
		return -E2BIG;
	/*
	 * The answers leave with mmap_lock released: writing one may fault (an output array not in memory yet), the fault
	 * takes mmap_lock again, and behind another thread that waits to write-lock it (an mmap()) that never returns.
	 */
	for (i = 0; i < n && !err; i += k) {
		mmap_read_lock(mm);
		for (k = 0; k < ARRAY_SIZE(pa) && i + k < n && !err; k++)
			err = hc_xlate_one(mm, a.va + ((i + k) << PAGE_SHIFT), &pa[k]);
		mmap_read_unlock(mm);
		if (!err && copy_to_user((u64 __user *)a.out + i, pa, k * sizeof(*pa)))
			err = -EFAULT;
	}
	return err;
}

static void hc_pin_free(struct hc_pin *p)
{
	unpin_user_pages_dirty_lock(p->pages, p->n, true);	/* the foreign kernel wrote them through its own mappings */
	kvfree(p->pages);
	kfree(p);
}

/* 1 when any part of the range is a device mapping or not mapped at all (the pin then fails by itself) */
static int hc_range_is_device(unsigned long va, unsigned long len)
{
	struct mm_struct *mm = current->mm;
	struct vm_area_struct *vma;
	unsigned long at = va;
	int dev = 0;

	mmap_read_lock(mm);
	while (at < va + len) {
		vma = vma_lookup(mm, at);
		if (!vma)
			break;
		if (vma->vm_flags & (VM_IO | VM_PFNMAP)) {
			dev = 1;
			break;
		}
		at = vma->vm_end;
	}
	mmap_read_unlock(mm);
	return dev;
}

static long hc_pin(struct hc_file *hf, unsigned int cmd, struct xnu_hostcall_range __user *ua)
{
	struct xnu_hostcall_range a;
	struct hc_pin *p, *found = NULL;
	long got;

	if (copy_from_user(&a, ua, sizeof(a)))
		return -EFAULT;
	if (cmd == XNU_HOSTCALL_UNPIN) {
		mutex_lock(&hf->lock);
		list_for_each_entry(p, &hf->pins, link)
			if (p->va == a.va) {
				found = p;
				list_del(&p->link);
				break;
			}
		mutex_unlock(&hf->lock);
		if (!found)
			return -ENOENT;
		hc_pin_free(found);
		return 0;
	}
	if (((a.va | a.len) & (PAGE_SIZE - 1)) || !a.len || (a.len >> PAGE_SHIFT) > HC_PIN_MAX)
		return -EINVAL;
	if (a.va + a.len < a.va)
		return -EINVAL;
	if (hc_range_is_device(a.va, a.len))
		return -EOPNOTSUPP;
	p = kzalloc(sizeof(*p), GFP_KERNEL);
	if (!p)
		return -ENOMEM;
	p->va = a.va;
	p->n = a.len >> PAGE_SHIFT;
	p->pages = kvmalloc_array(p->n, sizeof(*p->pages), GFP_KERNEL);
	if (!p->pages) {
		kfree(p);
		return -ENOMEM;
	}
	got = pin_user_pages_fast(p->va, p->n, FOLL_WRITE | FOLL_LONGTERM, p->pages);
	if (got != p->n) {
		if (got > 0)
			unpin_user_pages(p->pages, got);
		kvfree(p->pages);
		kfree(p);
		return got < 0 ? got : -EFAULT;
	}
	mutex_lock(&hf->lock);
	list_add(&p->link, &hf->pins);
	mutex_unlock(&hf->lock);
	return 0;
}

static long hc_ioctl(struct file *f, unsigned int cmd, unsigned long arg)
{
	if (cmd == XNU_HOSTCALL_XLATE)
		return hc_xlate((struct xnu_hostcall_xlate __user *)arg);
	if (cmd == XNU_HOSTCALL_PIN || cmd == XNU_HOSTCALL_UNPIN)
		return hc_pin(f->private_data, cmd, (struct xnu_hostcall_range __user *)arg);
	return -ENOTTY;
}

static int hc_dev_open(struct inode *i, struct file *f)
{
	struct hc_file *hf = kzalloc(sizeof(*hf), GFP_KERNEL);

	if (!hf)
		return -ENOMEM;
	mutex_init(&hf->lock);
	INIT_LIST_HEAD(&hf->pins);
	f->private_data = hf;
	spin_lock(&hc_lock);
	hc_open++;
	spin_unlock(&hc_lock);
	return 0;
}

static int hc_dev_release(struct inode *i, struct file *f)
{
	struct hc_file *hf = f->private_data;
	struct hc_pin *p, *next;

	list_for_each_entry_safe(p, next, &hf->pins, link)
		hc_pin_free(p);
	kfree(hf);
	spin_lock(&hc_lock);
	hc_open--;
	spin_unlock(&hc_lock);
	wake_up(&hc_done_wait);		/* a call waiting on this service fails now */
	return 0;
}

static const struct file_operations hc_fops = {
	.owner = THIS_MODULE,
	.open = hc_dev_open,
	.release = hc_dev_release,
	.read = hc_read,
	.write = hc_write,
	.unlocked_ioctl = hc_ioctl,
	.llseek = noop_llseek,
};

static struct miscdevice hc_dev = {
	.minor = MISC_DYNAMIC_MINOR,
	.name = "xnu-hostcall",
	.fops = &hc_fops,
	.mode = 0600,
};

int fki_hostcall_init(void)
{
	return misc_register(&hc_dev);
}

void fki_hostcall_exit(void)
{
	misc_deregister(&hc_dev);
	kvfree(hc_buf);
	hc_buf = NULL;
}
