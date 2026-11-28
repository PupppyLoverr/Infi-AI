// SPDX-License-Identifier: GPL-2.0-or-later
#include <metal_stdlib>
using namespace metal;

kernel void vecadd(
    device const float *a [[buffer(0)]],
    device const float *b [[buffer(1)]],
    device float *c       [[buffer(2)]],
    uint id               [[thread_position_in_grid]])
{
    c[id] = a[id] + b[id];
}
