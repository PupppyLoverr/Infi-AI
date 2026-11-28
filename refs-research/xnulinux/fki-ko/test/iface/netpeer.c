// SPDX-License-Identifier: GPL-2.0-only
/*
 * Linux's end of the link xnu0 for run.sh: a raw socket on the device, as any Linux program has one.
 *   netpeer IF send COUNT       send COUNT frames of the test (60, 61, 590 and 1514 bytes in turn, numbered)
 *   netpeer IF recv COUNT SECS  wait for COUNT frames of the test (at most SECS seconds) and check each one
 *   netpeer IF hold COUNT SECS  send COUNT frames, then watch what the kernel still charges this socket for them
 *                               (frames nobody has taken or freed) until that is nothing, at most SECS seconds
 * recv prints "listening" when it is; exit status 0 when all COUNT frames came, in order and whole. hold prints
 * "holding" when the frames are sent; exit status 0 when nothing is charged any more.
 * The frames are those of ifacetest's main.c: an ethertype for local experiments, a number, bytes that follow from it.
 */
#include <arpa/inet.h>
#include <errno.h>
#include <linux/if_packet.h>
#include <linux/sockios.h>
#include <net/if.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define TEST_PROTO 0x88b5
static const unsigned short lens[4] = { 60, 61, 590, 1514 };

static void frame_fill(unsigned char *f, unsigned seq, unsigned len)
{
	unsigned i;

	memset(f, 0xff, 6);
	memcpy(f + 6, "\x02xnut", 6);
	f[12] = TEST_PROTO >> 8; f[13] = TEST_PROTO & 0xff;
	for (i = 14; i < len; i++) f[i] = i < 18 ? seq >> (8 * (17 - i)) : seq * 13 + i;
}

static int frame_ok(const unsigned char *f, long len, unsigned seq)
{
	unsigned i;

	if (len != lens[seq % 4]) return 0;
	for (i = 14; i < len; i++) if (f[i] != (unsigned char)(i < 18 ? seq >> (8 * (17 - i)) : seq * 13 + i)) return 0;
	return 1;
}

int main(int argc, char **argv)
{
	struct sockaddr_ll ll = { .sll_family = AF_PACKET, .sll_protocol = htons(TEST_PROTO) };
	unsigned char f[2048];
	unsigned count, i, good = 0, got = 0, refused = 0;
	int s, big = 64 << 20;

	if (argc < 4 || (strcmp(argv[2], "send") && ((strcmp(argv[2], "recv") && strcmp(argv[2], "hold")) || argc < 5))) {
		fprintf(stderr, "usage: netpeer IF send COUNT | netpeer IF recv COUNT SECS | netpeer IF hold COUNT SECS\n");
		return 2;
	}
	count = strtoul(argv[3], NULL, 0);
	ll.sll_ifindex = if_nametoindex(argv[1]);
	s = socket(AF_PACKET, SOCK_RAW, htons(TEST_PROTO));
	if (s < 0 || !ll.sll_ifindex || bind(s, (struct sockaddr *)&ll, sizeof(ll))) { perror(argv[1]); return 2; }
	if (strcmp(argv[2], "recv")) {
		int charged = 0, ms;

		setsockopt(s, SOL_SOCKET, SO_SNDBUFFORCE, &big, sizeof(big));	/* frames nobody takes stay charged to the socket */
		for (i = 0; i < count; i++) {
			frame_fill(f, i, lens[i % 4]);
			if (send(s, f, lens[i % 4], MSG_DONTWAIT) != lens[i % 4]) refused++;
		}
		if (!strcmp(argv[2], "send")) {
			printf("sent %u frames on %s (%u refused)\n", count - refused, argv[1], refused);
			return refused ? 1 : 0;
		}
		ioctl(s, SIOCOUTQ, &charged);
		printf("holding: %u frames sent on %s, %d bytes charged to the socket for them\n", count - refused, argv[1], charged); fflush(stdout);
		for (ms = 0; charged && ms < atoi(argv[4]) * 1000; ms += 50) {
			usleep(50000);
			ioctl(s, SIOCOUTQ, &charged);
		}
		printf("%d ms later: %d bytes charged\n", ms, charged);
		return charged || refused ? 1 : 0;
	}
	time_t end = time(NULL) + atoi(argv[4]);
	puts("listening"); fflush(stdout);
	while (got < count && time(NULL) < end) {
		struct pollfd p = { s, POLLIN, 0 };
		long r;

		if (poll(&p, 1, 200) <= 0) continue;
		r = recv(s, f, sizeof(f), 0);
		if (r < 0) continue;
		if (frame_ok(f, r, got)) good++;
		got++;
	}
	printf("received %u of %u frames on %s, %u of them in order and whole\n", got, count, argv[1], good);
	return got == count && good == count ? 0 : 1;
}
