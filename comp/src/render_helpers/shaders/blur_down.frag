#version 100

precision highp float;

varying vec2 v_coords;

uniform sampler2D tex;
uniform vec2 half_pixel;
uniform float offset;
// 1.0 on the first downsample pass only: decode sRGB to linear so the
// blur averages light correctly; later passes already read linear data.
uniform float decode;

vec4 sample_tex(vec2 uv) {
    vec4 c = texture2D(tex, uv);
    if (decode > 0.5) {
        c.rgb = pow(c.rgb, vec3(2.2));
    }
    return c;
}

void main() {
    vec2 o = half_pixel * offset;

    vec4 sum = sample_tex(v_coords) * 4.0;
    sum += sample_tex(v_coords + vec2(-o.x, -o.y));
    sum += sample_tex(v_coords + vec2( o.x, -o.y));
    sum += sample_tex(v_coords + vec2(-o.x,  o.y));
    sum += sample_tex(v_coords + vec2( o.x,  o.y));

    gl_FragColor = sum / 8.0;
}
