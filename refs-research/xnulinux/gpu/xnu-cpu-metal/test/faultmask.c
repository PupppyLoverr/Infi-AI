// SPDX-License-Identifier: GPL-2.0-or-later
// What Darwin does with a hardware fault on a thread that has the fault's signal blocked: the signal stays pending, the
// thread runs the same instruction again and faults again, forever, at full CPU. A real Mac does the same. It is why
// patches/mesa/0003 leaves the fault signals unblocked on the threads Mesa starts: lavapipe's rasteriser
// threads had SIGBUS blocked, and one that jumped to non-executable memory never ended its process.
//
//   clang -o faultmask faultmask.c && ./faultmask
//
// A child process takes the fault on such a thread (a jump into heap memory: SIGBUS on Darwin); this process watches it
// for two seconds and kills it. Prints one line per case and exits 0 when both behave as on a Mac:
//   fault signals blocked:   still faulting after 2 s
//   fault signals unblocked: killed by signal 10
#include <stdio.h>
#include <stdlib.h>
#include <signal.h>
#include <pthread.h>
#include <unistd.h>
#include <sys/wait.h>

static int leave_fault_signals;

static void *worker(void *arg) {
  sigset_t set;
  sigfillset(&set);
  if (leave_fault_signals) { sigdelset(&set, SIGBUS); sigdelset(&set, SIGSEGV); sigdelset(&set, SIGILL); sigdelset(&set, SIGTRAP); }
  pthread_sigmask(SIG_BLOCK, &set, NULL);
  unsigned int *code = malloc(4096);
  for (int i = 0; i < 1024; i++) code[i] = 0xd65f03c0;   // ret
  ((void (*)(void))code)();                               // readable and writable, not executable
  return arg;
}

static int run(int leave) {   // 0: the child was still faulting after 2 s; otherwise the signal that ended it; -1: neither
  pid_t pid = fork();
  if (pid == 0) {
    leave_fault_signals = leave;
    pthread_t t;
    pthread_create(&t, NULL, worker, NULL);
    pthread_join(t, NULL);
    _exit(3);                                             // the jump returned: the heap was executable
  }
  int status = 0;
  for (int i = 0; i < 20; i++) {
    if (waitpid(pid, &status, WNOHANG) == pid) return WIFSIGNALED(status) ? WTERMSIG(status) : -1;
    usleep(100000);
  }
  kill(pid, SIGKILL);
  waitpid(pid, &status, 0);
  return 0;
}

int main(void) {
  int blocked = run(0), open = run(1);
  if (blocked == 0) printf("fault signals blocked:   still faulting after 2 s\n");
  else printf("fault signals blocked:   %s %d\n", blocked > 0 ? "killed by signal" : "ended, status", blocked);
  if (open > 0) printf("fault signals unblocked: killed by signal %d\n", open);
  else printf("fault signals unblocked: %s\n", open == 0 ? "still faulting after 2 s" : "ended without a signal");
  return blocked == 0 && open == SIGBUS ? 0 : 1;
}
