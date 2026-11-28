// SPDX-License-Identifier: GPL-2.0-or-later
#include <metal_stdlib>
using namespace metal;

struct VOut {
    float4 pos [[position]];
    float4 color;
};

vertex VOut tri_vertex(uint vid [[vertex_id]]) {
    const float2 pos[3] = {
        float2(-0.8, -0.8),
        float2( 0.8, -0.8),
        float2( 0.0,  0.8),
    };
    VOut o;
    o.pos = float4(pos[vid], 0.0, 1.0);
    o.color = float4(1.0, 0.2, 0.0, 1.0); // bright orange-red
    return o;
}

fragment float4 tri_fragment(VOut in [[stage_in]]) {
    return in.color;
}
