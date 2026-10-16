#version 440

layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;
layout(binding = 1) uniform sampler2D zoneColors;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 card;
    vec4 effect;
} ubuf;

vec3 edgeColor(float edge, float position)
{
    return texture(zoneColors, vec2(clamp(position, 0.0, 1.0), (edge + 0.5) * 0.25)).rgb;
}

void main()
{
    vec2 outerSize = ubuf.card.xy;
    vec2 p = qt_TexCoord0 * outerSize - vec2(ubuf.card.z);
    vec2 size = outerSize - vec2(2.0 * ubuf.card.z);
    if (size.x <= 0.0 || size.y <= 0.0) {
        fragColor = vec4(0.0);
        return;
    }

    float radius = min(max(0.0, ubuf.card.w), min(size.x, size.y) * 0.5);
    vec2 q = abs(p - size * 0.5) - (size * 0.5 - vec2(radius));
    float distanceToCard = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    if (distanceToCard <= 0.0) {
        fragColor = vec4(0.0);
        return;
    }

    float band = max(0.0, ubuf.effect.x);
    float reach = max(band + 0.001, ubuf.effect.y);
    float coverage = 1.0 - smoothstep(band, reach, distanceToCard);
    if (coverage <= 0.0) {
        fragColor = vec4(0.0);
        return;
    }

    float horizontalPosition = (p.x - 22.0 * (ubuf.card.z / 16.0))
                             / max(size.x - 44.0 * (ubuf.card.z / 16.0), 1.0);
    float verticalPosition = (p.y - 22.0 * (ubuf.card.z / 16.0))
                           / max(size.y - 44.0 * (ubuf.card.z / 16.0), 1.0);
    float horizontalEdge = p.y < size.y * 0.5 ? 0.0 : 2.0;
    float verticalEdge = p.x < size.x * 0.5 ? 3.0 : 1.0;
    vec3 color;
    if (q.x > 0.0 && q.y > 0.0) {
        // Project onto the rounded turn: edge-line distances reverse outside
        // the card and create pinched color wedges instead of a continuous arc.
        float turn = atan(q.x, q.y) * (2.0 / 3.141592653589793);
        color = mix(edgeColor(horizontalEdge, horizontalPosition),
                    edgeColor(verticalEdge, verticalPosition), turn);
    } else if (q.x > q.y) {
        color = edgeColor(verticalEdge, verticalPosition);
    } else {
        color = edgeColor(horizontalEdge, horizontalPosition);
    }

    float brightness = clamp(ubuf.effect.z, 0.0, 1.35);
    float runningOpacity = ubuf.effect.w > 0.5 ? 1.0 : 0.3;
    float alpha = coverage * 0.95 * runningOpacity * ubuf.qt_Opacity;
    color = clamp(color * brightness, 0.0, 1.0);
    fragColor = vec4(color * alpha, alpha);
}
