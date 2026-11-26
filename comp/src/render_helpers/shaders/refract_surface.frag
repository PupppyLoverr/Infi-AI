#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision highp float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform float niri_scale;

// Liquid-glass lensing (Solyra). lens: the glass shape's box in clip-geo
// coordinates normalised to [0, 1]; refraction: (strength px, corner radius px).
uniform vec4 lens;
uniform vec2 refraction;

// solyra_glass_v1 shapes: up to 4 rounded rectangles, each a box in clip-geo
// coordinates normalised to [0, 1]; their corner radii (px) and lens
// strengths (px, preset-scaled); how many are set. With none set, the single
// `lens` box above is the fallback.
uniform vec4 shape0;
uniform vec4 shape1;
uniform vec4 shape2;
uniform vec4 shape3;
uniform vec4 shape_radius;
uniform vec4 shape_lens;
uniform float shape_count;

uniform vec2 geo_size;
uniform vec4 corner_radius;
uniform mat3 input_to_geo;

float niri_rounding_alpha(vec2 coords, vec2 size, vec4 corner_radius);
vec4 postprocess(vec4 color);

float lens_sdf(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

// Signed distance (px) from p to one normalised rounded box, and the outward
// normal there.
float box_sdf(vec4 box, float radius, vec2 p, out vec2 n) {
    vec2 half_size = box.zw * geo_size * 0.5;
    vec2 q = p - (box.xy * geo_size + half_size);
    float r = min(radius, min(half_size.x, half_size.y));
    float d = lens_sdf(q, half_size, r);
    vec2 g = vec2(
        lens_sdf(q + vec2(1.0, 0.0), half_size, r) - lens_sdf(q - vec2(1.0, 0.0), half_size, r),
        lens_sdf(q + vec2(0.0, 1.0), half_size, r) - lens_sdf(q - vec2(0.0, 1.0), half_size, r));
    n = length(g) > 1e-5 ? normalize(g) : vec2(0.0);
    return d;
}

// Keep the nearer shape. Strict `<` keeps the earlier shape on ties, so
// overlaps resolve deterministically to the shape added first.
void nearer(vec4 box, float radius, float strength, vec2 p,
            inout float best, inout vec2 best_n, inout float best_k) {
    vec2 n;
    float d = box_sdf(box, radius, p, n);
    if (d < best) {
        best = d;
        best_n = n;
        best_k = strength;
    }
}

void main() {
    vec3 coords_geo = input_to_geo * vec3(v_coords, 1.0);

    // Sample the texture, bent at the edge of the glass like a thick convex
    // lens: near the rim the backdrop is sampled from further inside, so the
    // scene behind appears to curve over the edge, with a slight RGB split.
    // Sampling inward never leaves the captured backdrop.
    vec2 p = coords_geo.xy * geo_size;
    float glass_cov = 1.0;
    float d = 1.0;
    vec2 n = vec2(0.0);
    float k = 0.0;
    if (shape_count > 0.5) {
        // Analytic shapes (solyra_glass_v1): the union of the boxes is the
        // glass; lensing follows the nearest edge.
        float best = 1e9;
        nearer(shape0, shape_radius.x, shape_lens.x, p, best, n, k);
        if (shape_count > 1.5) nearer(shape1, shape_radius.y, shape_lens.y, p, best, n, k);
        if (shape_count > 2.5) nearer(shape2, shape_radius.z, shape_lens.z, p, best, n, k);
        if (shape_count > 3.5) nearer(shape3, shape_radius.w, shape_lens.w, p, best, n, k);
        d = best;
        // One logical pixel of antialiasing on the shape's edge.
        glass_cov = clamp(0.5 - d, 0.0, 1.0);
    } else if (refraction.x > 0.0 && lens.z > 0.0 && lens.w > 0.0) {
        // Fallback: one box from the blur region's bounds.
        d = box_sdf(lens, refraction.y, p, n);
        k = refraction.x;
    }

    vec4 color;
    float band = max(k * 1.5, 6.0);
    // Only inside the glass, strongest at the rim, fading inward.
    float f = (k <= 0.0 || d > 0.0) ? 0.0 : pow(clamp(1.0 + d / band, 0.0, 1.0), 2.0);
    if (f > 0.0) {
        // Pixels in clip-geo space to texture coordinates.
        vec2 per_tex = vec2(input_to_geo[0][0], input_to_geo[1][1]) * geo_size;
        vec2 off = -n * f * k / per_tex;
        vec4 cg = texture2D(tex, v_coords + off);
        float cr = texture2D(tex, v_coords + off * 1.25).r;
        float cb = texture2D(tex, v_coords + off * 0.75).b;
        color = vec4(cr, cg.g, cb, cg.a);
    } else {
        color = texture2D(tex, v_coords);
    }
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif

    color = postprocess(color);

    if (coords_geo.x < 0.0 || 1.0 < coords_geo.x || coords_geo.y < 0.0 || 1.0 < coords_geo.y) {
        // Clip outside geometry.
        color = vec4(0.0);
    } else {
        // Apply corner rounding inside geometry.
        color = color * niri_rounding_alpha(coords_geo.xy * geo_size, geo_size, corner_radius);
    }

    // Clip to the analytic glass shapes (1.0 without any).
    color = color * glass_cov;

    // Apply final alpha and tint.
    color = color * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
