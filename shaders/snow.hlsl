// Instanced soft discs: one float2 centre per instance, corners from SV_VertexID.
cbuffer Frame : register(b0)
{
    float2 clip_scale;   // 2 / back-buffer width, 2 / back-buffer height
    float  radius;       // disc radius in back-buffer pixels
    float  soft;         // half-width of the soft edge in back-buffer pixels
    float4 color;        // straight RGB, alpha unused
};

struct VSOut
{
    float4 pos : SV_Position;
    float2 off : TEXCOORD0;   // pixel offset from the disc centre
};

VSOut vs_main(float2 center : POSITION, uint vid : SV_VertexID)
{
    float2 corner = float2((vid & 1) ? 1.0 : -1.0, (vid & 2) ? 1.0 : -1.0);
    float2 off = corner * (radius + soft);
    float2 p = center + off;
    VSOut o;
    o.pos = float4(p.x * clip_scale.x - 1.0, 1.0 - p.y * clip_scale.y, 0.0, 1.0);
    o.off = off;
    return o;
}

float4 ps_main(VSOut i) : SV_Target
{
    float d = length(i.off);
    float t = saturate((radius + soft - d) / (2.0 * soft));
    float a = t * t * (3.0 - 2.0 * t);
    return float4(color.rgb * a, a);
}
