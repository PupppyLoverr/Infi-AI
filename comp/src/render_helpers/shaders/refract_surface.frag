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
uniform vec4 shape_preset;
uniform float shape_count;
// Accessibility: refraction / frost / contrast scales (1.0 = native).
uniform vec4 mat_intensity;

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
void nearer(vec4 box, float radius, float strength, float preset, vec2 p,
            inout float best, inout vec2 best_n, inout float best_k, inout float best_p) {
    vec2 n;
    float d = box_sdf(box, radius, p, n);
    if (d < best) {
        best = d;
        best_n = n;
        best_k = strength;
        best_p = preset;
    }
}

// Material parameters per preset (solyra_glass_v1): tint colour and
// strength, rim-light intensity, specular intensity, inner shade, frost
// (extra milky desaturation). Tuned against the macOS reference.
void material_for(float preset, out vec3 tint, out float tint_str,
                  out float rim, out float spec, out float shade, out float frost) {
    tint = vec3(1.0);
    tint_str = 0.05;
    rim = 0.30;
    spec = 0.45;
    shade = 0.10;
    frost = 0.0;
    if (preset > 0.5 && preset < 1.5) {        // regular
        tint_str = 0.12; rim = 0.25; spec = 0.30; shade = 0.14; frost = 0.18;
    } else if (preset > 1.5 && preset < 2.5) { // solid
        tint = vec3(0.92); tint_str = 0.55; rim = 0.08; spec = 0.04; shade = 0.05;
    } else if (preset > 2.5 && preset < 3.5) { // dark
        tint = vec3(0.02); tint_str = 0.30; rim = 0.30; spec = 0.35; shade = 0.10;
    } else if (preset > 3.5 && preset < 4.5) { // dock: adaptive strength below
        tint_str = 0.10; rim = 0.35; spec = 0.50; shade = 0.12;
    } else if (preset > 4.5 && preset < 5.5) { // island
        tint = vec3(0.02); tint_str = 0.42; rim = 0.35; spec = 0.45; shade = 0.08;
    } else if (preset > 5.5 && preset < 6.5) { // popover
        tint_str = 0.18; rim = 0.22; spec = 0.25; shade = 0.12; frost = 0.25;
    } else if (preset > 6.5) {                 // window
        tint_str = 0.06; rim = 0.18; spec = 0.20; shade = 0.08; frost = 0.08;
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
    float prst = -1.0;
    if (shape_count > 0.5) {
        // Analytic shapes (solyra_glass_v1): the union of the boxes is the
        // glass; lensing follows the nearest edge.
        float best = 1e9;
        nearer(shape0, shape_radius.x, shape_lens.x, shape_preset.x, p, best, n, k, prst);
        if (shape_count > 1.5) nearer(shape1, shape_radius.y, shape_lens.y, shape_preset.y, p, best, n, k, prst);
        if (shape_count > 2.5) nearer(shape2, shape_radius.z, shape_lens.z, shape_preset.z, p, best, n, k, prst);
        if (shape_count > 3.5) nearer(shape3, shape_radius.w, shape_lens.w, shape_preset.w, p, best, n, k, prst);
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
    // Material pass (solyra_glass_v1 shapes only): adaptive tint, frost,
    // rim light, specular streak and inner shade, all inside the glass.
    if (shape_count > 0.5 && prst >= 0.0 && d < 0.0) {
        vec3 tint;
        float tint_str, rim, spec, shade, frost;
        material_for(prst, tint, tint_str, rim, spec, shade, frost);
        float frost_s = max(mat_intensity.y, 0.0);
        float contrast_s = max(mat_intensity.z, 0.5);
        tint_str *= mix(1.0, contrast_s, 0.5);
        frost *= frost_s;
        rim *= mix(1.0, contrast_s, 0.35);

        // Backdrop luminance, no readback: the texture is already the
        // blurred backdrop, so a few taps inside the shape estimate it.
        vec2 per_tex2 = vec2(input_to_geo[0][0], input_to_geo[1][1]) * geo_size;
        vec2 tap = vec2(24.0) / max(per_tex2, vec2(1.0));
        vec3 acc = texture2D(tex, v_coords).rgb
            + texture2D(tex, v_coords + vec2(tap.x, 0.0)).rgb
            + texture2D(tex, v_coords - vec2(tap.x, 0.0)).rgb
            + texture2D(tex, v_coords + vec2(0.0, tap.y)).rgb
            + texture2D(tex, v_coords - vec2(0.0, tap.y)).rgb;
        float luma = dot(acc / 5.0, vec3(0.2126, 0.7152, 0.0722));

        // Adaptive tint: over bright backdrops darken a touch, over dark
        // ones lighten, so text on the glass stays legible either way.
        float adapt = (luma - 0.5) * 2.0; // -1 dark scene .. +1 bright scene
        vec3 tint_col = tint;
        float str = tint_str;
        if (tint == vec3(1.0)) {
            // Light materials adapt by darkening instead of going opaque.
            tint_col = mix(vec3(1.0), vec3(0.05), clamp(adapt, 0.0, 1.0) * 0.55);
        }
        color.rgb = mix(color.rgb, tint_col * clamp(luma + 0.55, 0.4, 1.3), str);

        // Frost: milky desaturation lifting the backdrop toward its luma.
        if (frost > 0.0) {
            float g = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
            color.rgb = mix(color.rgb, vec3(g) * 1.06 + 0.04, frost);
        }

        // Rim light: a bright hairline just inside the edge, lit from
        // top-left; inner shade darkens bottom-facing edges. d<0 inside.
        float edge_band = smoothstep(-3.0, 0.0, d);
        float lit = clamp(dot(n, normalize(vec2(-0.6, -0.8))) * 0.5 + 0.5, 0.0, 1.0);
        color.rgb += rim * edge_band * lit * 0.35;
        color.rgb -= shade * smoothstep(-6.0, 0.0, d) * clamp(n.y, 0.0, 1.0) * 0.25;

        // Specular streak: thin bright line on top-facing edges only.
        float spec_band = smoothstep(-1.5, 0.0, d) * clamp(-n.y, 0.0, 1.0);
        color.rgb += spec * spec_band * 0.30;
        color.rgb = clamp(color.rgb, 0.0, 1.0);
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
