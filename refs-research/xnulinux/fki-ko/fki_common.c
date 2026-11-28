// SPDX-License-Identifier: GPL-2.0-only
/*
 * Foreign kernel interface, version 1 (architecture-independent core).
 * See fki.h.
 *
 * Exports are plain EXPORT_SYMBOL on purpose: this file defines a narrow,
 * stable, semantic interface of its own, and nothing behind it is
 * re-exported.
 */
#include "fki.h"
#include "fki_internal.h"
#include <linux/cpumask.h>
#include <linux/delay.h>
#include <linux/err.h>
#include <linux/etherdevice.h>
#include <linux/firmware.h>
#include <linux/fs.h>
#include <linux/hrtimer.h>
#include <linux/irqflags.h>
#include <linux/kthread.h>
#include <linux/math64.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/netdevice.h>
#include <linux/preempt.h>
#include <linux/printk.h>
#include <linux/random.h>
#include <linux/sched.h>
#include <linux/skbuff.h>
#include <linux/slab.h>
#include <linux/smp.h>
#include <linux/timekeeping.h>

int fki_version(void)
{
	return FKI_VERSION;
}
EXPORT_SYMBOL(fki_version);

void fki_log(const char *s, unsigned long len)
{
	pr_info("fki: %.*s\n", (int)len, s);
}
EXPORT_SYMBOL(fki_log);

/* what an operator must see even with the console quiet (an XNU panic, a CPU gone silent): KERN_ALERT */
void fki_log_alert(const char *s, unsigned long len)
{
	pr_alert("fki: %.*s\n", (int)len, s);
}
EXPORT_SYMBOL(fki_log_alert);

int fki_blob_get(const char *name, const void **data, unsigned long *size, void **handle)
{
	const struct firmware *fw;
	int err = request_firmware(&fw, name, NULL);

	if (err)
		return err;
	*data = fw->data;
	*size = fw->size;
	*handle = (void *)fw;
	return 0;
}
EXPORT_SYMBOL(fki_blob_get);

void fki_blob_put(void *handle)
{
	release_firmware(handle);
}
EXPORT_SYMBOL(fki_blob_put);

void fki_random(void *buf, unsigned long len)
{
	get_random_bytes(buf, len);
}
EXPORT_SYMBOL(fki_random);

int fki_random_wait(void)
{
	return wait_for_random_bytes();
}
EXPORT_SYMBOL(fki_random_wait);

static int cpu_list[NR_CPUS];
static int cpu_list_n;
module_param_array(cpu_list, int, &cpu_list_n, 0444);

int fki_cpu_map(int cpu)
{
	if (!cpu_list_n)
		return cpu;
	return cpu >= 0 && cpu < cpu_list_n ? cpu_list[cpu] : -1;
}

int fki_bind_cpu(int cpu)
{
	cpu = fki_cpu_map(cpu);
	if (cpu < 0 || cpu >= nr_cpu_ids || !cpu_online(cpu))
		return -EINVAL;
	return set_cpus_allowed_ptr(current, cpumask_of(cpu));
}
EXPORT_SYMBOL(fki_bind_cpu);

int fki_num_cpus(void)
{
	return cpu_list_n ? cpu_list_n : num_online_cpus();
}
EXPORT_SYMBOL(fki_num_cpus);

void *fki_thread_start(int (*fn)(void *), void *arg, int cpu, const char *name)
{
	struct task_struct *t;

	cpu = fki_cpu_map(cpu);
	if (cpu < 0 || cpu >= nr_cpu_ids || !cpu_online(cpu))
		return NULL;
	t = kthread_create(fn, arg, "%s", name);
	if (IS_ERR(t))
		return NULL;
	kthread_bind(t, cpu);
	get_task_struct(t);
	wake_up_process(t);
	return t;
}
EXPORT_SYMBOL(fki_thread_start);

void fki_thread_stop(void *thread)
{
	if (thread) {
		kthread_stop(thread);
		put_task_struct(thread);
	}
}
EXPORT_SYMBOL(fki_thread_stop);

int fki_thread_should_stop(void)
{
	return kthread_should_stop();
}
EXPORT_SYMBOL(fki_thread_should_stop);

void fki_thread_wait(const volatile unsigned long *flag)
{
	for (;;) {
		set_current_state(TASK_INTERRUPTIBLE);
		if (READ_ONCE(*flag) || kthread_should_stop())
			break;
		schedule();
	}
	__set_current_state(TASK_RUNNING);
}
EXPORT_SYMBOL(fki_thread_wait);

void fki_thread_wake(void *thread)
{
	if (thread)
		wake_up_process(thread);
}
EXPORT_SYMBOL(fki_thread_wake);

void fki_msleep(unsigned int ms)
{
	unsigned long t = msecs_to_jiffies(ms) + 1;

	while (t)
		t = schedule_timeout_idle(t);
}
EXPORT_SYMBOL(fki_msleep);

/* ---- running Linux from inside fki_enter() ---- */

struct fki_runs {
	void (*fn)(void *);
	void *arg;
};

/* fn runs with interrupts on and turns them off again; then, before the foreign kernel resumes, fki_fpsimd_forget */
static void fki_runs_then_forget(void *p)
{
	struct fki_runs *r = p;

	r->fn(r->arg);
	local_irq_enable();
	fki_fpsimd_forget();
	local_irq_disable();
}

static void fki_linux_runs(void (*fn)(void *), void *arg)
{
	struct fki_runs r = { fn, arg };

	fki_arch_linux_runs(fki_runs_then_forget, &r);
}

void fki_linux_runs_ext(void (*fn)(void *), void *arg)
{
	fki_linux_runs(fn, arg);
}

struct fki_idle_args { fki_u64 ns; const volatile unsigned int *wake; };

static struct task_struct *fki_idle_task[NR_CPUS];	/* the task sleeping in fki_idle_until on each CPU, for wakeups */

static void fki_idle_body(void *p)
{
	struct fki_idle_args *a = p;
	ktime_t t = ns_to_ktime(a->ns);

	local_irq_enable();
	preempt_enable();
	set_current_state(TASK_IDLE);
	WRITE_ONCE(fki_idle_task[smp_processor_id()], current);
	smp_mb();	/* pairs with fki_kick_cpu: it sees this task, or this sees the word it set first */
	if (!a->wake || !READ_ONCE(*a->wake))
		schedule_hrtimeout_range(&t, 50000, HRTIMER_MODE_REL);
	__set_current_state(TASK_RUNNING);
	WRITE_ONCE(fki_idle_task[smp_processor_id()], NULL);
	preempt_disable();
	local_irq_disable();
}

static void fki_yield_body(void *p)
{
	local_irq_enable();
	preempt_enable();
	cond_resched();
	preempt_disable();
	local_irq_disable();
}

void fki_idle_until(fki_u64 counter_deadline, fki_u64 max_ns, const volatile unsigned int *wake)
{
	u64 now = fki_counter(), freq = fki_counter_frequency(), ns;
	struct fki_idle_args a;

	if (counter_deadline <= now) {
		/* already due: still let Linux take its interrupts and reschedule (an idle loop that never sleeps starves it) */
		fki_linux_runs(fki_yield_body, NULL);
		return;
	}
	ns = div64_u64((counter_deadline - now) * 1000000ULL, freq) * 1000ULL;
	if (ns > max_ns)
		ns = max_ns;
	a.ns = ns;
	a.wake = wake;
	fki_linux_runs(fki_idle_body, &a);
}
EXPORT_SYMBOL(fki_idle_until);

void fki_yield(void)
{
	fki_linux_runs(fki_yield_body, NULL);
}
EXPORT_SYMBOL(fki_yield);

/*
 * Make the foreign kernel on a CPU look at what another CPU left for it: wake it if it sleeps in fki_idle_until,
 * otherwise interrupt that CPU (its user code runs with interrupts on; its kernel looks where it enables its own).
 * Callable with interrupts off.
 */
static atomic_t fki_kick_claimed[NR_CPUS];
static void fki_kick_fn(void *unused) { atomic_set_release(&fki_kick_claimed[smp_processor_id()], 0); }
static DEFINE_PER_CPU(call_single_data_t, fki_kick_csd);

void fki_kick_cpu(int cpu)
{
	struct task_struct *t;

	cpu = fki_cpu_map(cpu);
	if (cpu < 0 || cpu >= nr_cpu_ids)
		return;
	smp_mb();	/* the caller's pending word before the idle task: pairs with fki_idle_body */
	t = READ_ONCE(fki_idle_task[cpu]);
	if (t)
		wake_up_process(t);
	else if (cpu != smp_processor_id() && atomic_cmpxchg_acquire(&fki_kick_claimed[cpu], 0, 1) == 0) {
		/* irq_work_queue_on is not exported; an IPI already on its way (claimed) is just as good */
		if (smp_call_function_single_async(cpu, per_cpu_ptr(&fki_kick_csd, cpu)))
			atomic_set_release(&fki_kick_claimed[cpu], 0);
	}
}
EXPORT_SYMBOL(fki_kick_cpu);

void fki_realtime(fki_u64 *sec, fki_u64 *nsec)
{
	struct timespec64 ts;

	ktime_get_real_ts64(&ts);
	*sec = ts.tv_sec;
	*nsec = ts.tv_nsec;
}
EXPORT_SYMBOL(fki_realtime);

/*
 * A virtual Ethernet link to the foreign kernel: the Linux netdev "xnu0". What Linux transmits on xnu0 is queued for
 * the foreign kernel (fki_net_recv); what the foreign kernel sends (fki_net_send) arrives on xnu0 as received frames.
 */
#define FKI_NET_QLEN 1024
static struct net_device *fki_netdev;
static struct sk_buff_head fki_net_rxq;	/* Linux -> foreign */

static netdev_tx_t fki_net_xmit(struct sk_buff *skb, struct net_device *dev)
{
	int first = fki_cpu_map(0);	/* the foreign kernel's first CPU: Linux's CPU 0, or the first of cpu_list */
	struct task_struct *t;

	if (skb_queue_len(&fki_net_rxq) >= FKI_NET_QLEN) {
		dev->stats.tx_dropped++;
		dev_kfree_skb_any(skb);
		return NETDEV_TX_OK;
	}
	dev->stats.tx_packets++;
	dev->stats.tx_bytes += skb->len;
	skb_queue_tail(&fki_net_rxq, skb);
	t = first >= 0 && first < nr_cpu_ids ? READ_ONCE(fki_idle_task[first]) : NULL;
	if (t)
		wake_up_process(t);	/* the foreign kernel's first CPU is idle: let it see the frame now */
	return NETDEV_TX_OK;
}

static int fki_net_up(struct net_device *dev) { netif_start_queue(dev); return 0; }
static int fki_net_down(struct net_device *dev) { netif_stop_queue(dev); return 0; }

static const struct net_device_ops fki_net_ops = {
	.ndo_open = fki_net_up,
	.ndo_stop = fki_net_down,
	.ndo_start_xmit = fki_net_xmit,
	.ndo_set_mac_address = eth_mac_addr,
	.ndo_validate_addr = eth_validate_addr,
};

struct fki_net_io { const void *in; void *out; unsigned long len; long result; };

static void fki_net_send_body(void *p)
{
	struct fki_net_io *io = p;
	struct sk_buff *skb;

	io->result = -ENODEV;
	if (!fki_netdev || io->len < ETH_HLEN || io->len > ETH_FRAME_LEN + 4)
		return;
	skb = netdev_alloc_skb(fki_netdev, io->len + NET_IP_ALIGN);
	if (!skb) {
		fki_netdev->stats.rx_dropped++;
		io->result = -ENOMEM;
		return;
	}
	skb_reserve(skb, NET_IP_ALIGN);
	skb_put_data(skb, io->in, io->len);
	skb->protocol = eth_type_trans(skb, fki_netdev);
	fki_netdev->stats.rx_packets++;
	fki_netdev->stats.rx_bytes += io->len;
	local_irq_enable();	/* netif_rx re-enables bottom halves, which must not happen with interrupts off */
	netif_rx(skb);
	local_irq_disable();
	io->result = io->len;
}

long fki_net_send(const void *frame, unsigned long len)
{
	struct fki_net_io io = { .in = frame, .len = len };

	fki_linux_runs(fki_net_send_body, &io);
	return io.result;
}
EXPORT_SYMBOL(fki_net_send);

static void fki_net_recv_body(void *p)
{
	struct fki_net_io *io = p;
	struct sk_buff *skb = skb_dequeue(&fki_net_rxq);

	io->result = 0;
	if (!skb)
		return;
	if (skb->len > io->len) {
		io->result = -EMSGSIZE;
	} else if (skb_copy_bits(skb, 0, io->out, skb->len) == 0) {
		io->result = skb->len;
	} else {
		io->result = -EIO;
	}
	dev_consume_skb_any(skb);
}

long fki_net_recv(void *buf, unsigned long len)
{
	struct fki_net_io io = { .out = buf, .len = len };

	fki_linux_runs(fki_net_recv_body, &io);
	return io.result;
}
EXPORT_SYMBOL(fki_net_recv);

long fki_net_pending(void)
{
	return skb_queue_len(&fki_net_rxq);
}
EXPORT_SYMBOL(fki_net_pending);

/* ---- host file backing store ---- */

void *fki_file_open(const char *path, int writable, fki_u64 *size)
{
	struct file *f = filp_open(path, (writable ? O_RDWR : O_RDONLY) | O_LARGEFILE, 0);

	if (IS_ERR(f))
		return NULL;
	if (size)	/* the mapping's host: a block device's own size (a device node's inode says 0) */
		*size = i_size_read(f->f_mapping->host);
	return f;
}
EXPORT_SYMBOL(fki_file_open);

#define FKI_FILE_CHUNK (256UL << 10)

static long fki_file_rw_body(void *file, int write, void *buf, unsigned long len, fki_u64 off);
struct fki_file_args { void *file; int write; void *buf; unsigned long len; fki_u64 off; long ret; };
static void fki_file_rw_thunk(void *p)
{
	struct fki_file_args *a = p;
	a->ret = fki_file_rw_body(a->file, a->write, a->buf, a->len, a->off);
}

long fki_file_rw(void *file, int write, void *buf, unsigned long len, fki_u64 off)
{
	struct fki_file_args a = { file, write, buf, len, off, -EIO };

	if (!file || (write && !(((struct file *)file)->f_mode & FMODE_WRITE)))
		return -EBADF;		/* (a file opened to read: kernel_write() says so with a WARN) */
	fki_linux_runs(fki_file_rw_thunk, &a);
	return a.ret;
}
EXPORT_SYMBOL(fki_file_rw);

static long fki_file_rw_body(void *file, int write, void *buf, unsigned long len, fki_u64 off)
{
	struct file *f = file;
	unsigned long done = 0;
	long ret = 0;
	void *bounce;

	local_irq_enable();
	preempt_enable();
	bounce = kvmalloc(min(len, FKI_FILE_CHUNK), GFP_KERNEL);
	if (!bounce)
		ret = -ENOMEM;
	while (bounce && done < len) {
		unsigned long n = min(len - done, FKI_FILE_CHUNK);
		loff_t pos = off + done;
		ssize_t r;

		if (write) {
			preempt_disable(); local_irq_disable();
			memcpy(bounce, (char *)buf + done, n);
			local_irq_enable(); preempt_enable();
			r = kernel_write(f, bounce, n, &pos);
		} else {
			r = kernel_read(f, bounce, n, &pos);
			if (r > 0) {
				preempt_disable(); local_irq_disable();
				memcpy((char *)buf + done, bounce, r);
				local_irq_enable(); preempt_enable();
			}
		}
		if (r < 0) {
			ret = r;
			break;
		}
		done += r;
		if ((unsigned long)r < n)
			break;
	}
	kvfree(bounce);
	preempt_disable();
	local_irq_disable();
	return ret < 0 && !done ? ret : (long)done;
}

long fki_file_pwrite(void *file, const void *buf, unsigned long len, fki_u64 off)
{
	loff_t pos = off;

	if (!file || !(((struct file *)file)->f_mode & FMODE_WRITE))
		return -EBADF;
	return kernel_write(file, buf, len, &pos);
}
EXPORT_SYMBOL(fki_file_pwrite);

void fki_file_close(void *file)
{
	if (file)
		filp_close(file, NULL);
}
EXPORT_SYMBOL(fki_file_close);

/* ---- module init / exit ---- */

static int __init fki_init(void)
{
	struct net_device *dev;
	int err, cpu;

	for_each_possible_cpu(cpu)
		INIT_CSD(per_cpu_ptr(&fki_kick_csd, cpu), fki_kick_fn, NULL);
	err = fki_arch_init();
	if (err)
		return err;
	skb_queue_head_init(&fki_net_rxq);
	err = fki_hostcall_init();
	if (err)
		pr_warn("fki: no /dev/xnu-hostcall (%d)\n", err);
	dev = alloc_etherdev(0);
	if (!dev)
		return 0;	/* no link; the rest of fki still works */
	strscpy(dev->name, "xnu0", IFNAMSIZ);
	dev->netdev_ops = &fki_net_ops;
	eth_hw_addr_random(dev);
	dev->min_mtu = 68;
	dev->max_mtu = ETH_DATA_LEN;
	err = register_netdev(dev);
	if (err) {
		free_netdev(dev);
		pr_warn("fki: no xnu0 link (%d)\n", err);
		return 0;
	}
	fki_netdev = dev;
	return 0;
}

static void __exit fki_exit(void)
{
	fki_hostcall_exit();
	if (fki_netdev) {
		unregister_netdev(fki_netdev);
		free_netdev(fki_netdev);
		fki_netdev = NULL;
	}
	skb_queue_purge(&fki_net_rxq);
	fki_arch_exit();
}
module_init(fki_init);
module_exit(fki_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Foreign kernel interface v1");
