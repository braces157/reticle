import { observeVisibility, prefersReducedMotion, Spring, SPRING_GENTLE } from './spring';

const VERT_SRC = `
attribute vec2 a_pos;
varying vec2 v_uv;
void main() {
  v_uv = a_pos * 0.5 + 0.5;
  gl_Position = vec4(a_pos, 0.0, 1.0);
}
`;

const FRAG_SRC = `
precision highp float;
varying vec2 v_uv;

uniform vec2 u_res;
uniform vec2 u_mouse;
uniform float u_time;
uniform float u_hover;
uniform vec2 u_ripple_pos;
uniform float u_ripple_t;

float gridLine(vec2 p, float spacing, float width) {
  vec2 g = abs(fract(p / spacing - 0.5) - 0.5) * spacing;
  float d = min(g.x, g.y);
  return 1.0 - smoothstep(0.0, width, d);
}

void main() {
  vec2 px = v_uv * u_res;
  vec2 center = u_res * vec2(0.5, 0.14);

  // Subtle optical lens refraction around spring-tracked cursor
  vec2 mpx = u_mouse * u_res;
  vec2 toMouse = px - mpx;
  float mDist = length(toMouse);
  float lensRadius = 260.0;
  float lensFalloff = exp(-(mDist * mDist) / (lensRadius * lensRadius)) * u_hover;
  vec2 refractedPx = px - normalize(toMouse + vec2(0.0001)) * lensFalloff * 14.0;

  // Optical shutter shockwave ripple on click
  vec2 rpx = u_ripple_pos * u_res;
  float rDist = length(px - rpx);
  float waveFront = u_ripple_t * 920.0;
  float waveMask = exp(-pow((rDist - waveFront) / 48.0, 2.0)) * max(0.0, 1.0 - u_ripple_t * 1.15);
  refractedPx += normalize(px - rpx + vec2(0.0001)) * waveMask * 10.0;

  // Fine calibration micro-grid & Leica cross-ticks
  float majorGrid = gridLine(refractedPx, 72.0, 1.05);
  float minorGrid = gridLine(refractedPx, 24.0, 0.85);

  // Overhead studio dome falloff
  vec2 domeVec = (px - center) / vec2(u_res.x * 0.58, u_res.y * 0.62);
  float dome = exp(-dot(domeVec, domeVec) * 1.35);

  // Chromatic dispersion caustics near cursor lens & ripple
  float chromaR = exp(-pow((mDist - 46.0) / 88.0, 2.0)) * u_hover * 0.14;
  float chromaB = exp(-pow((mDist - 62.0) / 104.0, 2.0)) * u_hover * 0.08;
  float breath = 0.88 + 0.12 * sin(u_time * 0.7);

  vec3 col = vec3(0.0);

  // Subtle overhead brushed titanium light + warm vermilion hint
  col += vec3(0.11, 0.12, 0.16) * dome * 0.55 * breath;
  col += vec3(1.0, 0.31, 0.188) * dome * 0.065;

  // Grid illumination concentrated under dome, cursor lens, and ripple wavefront
  float gridEnergy = dome * 0.16 + lensFalloff * 0.52 + waveMask * 0.72;
  col += vec3(0.86, 0.89, 0.96) * majorGrid * gridEnergy * 0.22;
  col += vec3(0.65, 0.70, 0.82) * minorGrid * (lensFalloff * 0.18 + waveMask * 0.28) * 0.22;

  // Specular crosshair axes passing through cursor lens
  float axisX = exp(-abs(px.y - mpx.y) * 0.8) * exp(-abs(px.x - mpx.x) / 240.0) * u_hover;
  float axisY = exp(-abs(px.x - mpx.x) * 0.8) * exp(-abs(px.y - mpx.y) / 240.0) * u_hover;
  col += vec3(1.0, 0.42, 0.30) * (axisX + axisY) * 0.18;

  // Optical lens caustic tint + ripple ring glow
  col += vec3(1.0, 0.31, 0.188) * (chromaR + waveMask * 0.28);
  col += vec3(0.22, 0.74, 0.97) * (chromaB + waveMask * 0.14);

  // Smooth vignette fade into #050507 void at bottom edge
  float bottomFade = smoothstep(0.0, 0.24, v_uv.y);
  col *= bottomFade;

  float alpha = clamp(max(max(col.r, col.g), col.b) * 1.45, 0.0, 0.95);
  gl_FragColor = vec4(col, alpha);
}
`;

export function initOpticalShader(): void {
  const canvas = document.getElementById('hero-optical-canvas') as HTMLCanvasElement | null;
  const hero = document.getElementById('top');
  if (!canvas || !hero || prefersReducedMotion()) return;

  const gl = canvas.getContext('webgl', {
    alpha: true,
    antialias: false,
    depth: false,
    stencil: false,
    powerPreference: 'high-performance',
  });
  if (!gl) return;

  const compile = (type: number, src: string): WebGLShader | null => {
    const shader = gl.createShader(type);
    if (!shader) return null;
    gl.shaderSource(shader, src);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      gl.deleteShader(shader);
      return null;
    }
    return shader;
  };

  const vs = compile(gl.VERTEX_SHADER, VERT_SRC);
  const fs = compile(gl.FRAGMENT_SHADER, FRAG_SRC);
  if (!vs || !fs) return;

  const program = gl.createProgram();
  if (!program) return;
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return;
  gl.useProgram(program);

  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW,
  );
  const posLoc = gl.getAttribLocation(program, 'a_pos');
  gl.enableVertexAttribArray(posLoc);
  gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

  const uRes = gl.getUniformLocation(program, 'u_res');
  const uMouse = gl.getUniformLocation(program, 'u_mouse');
  const uTime = gl.getUniformLocation(program, 'u_time');
  const uHover = gl.getUniformLocation(program, 'u_hover');
  const uRipplePos = gl.getUniformLocation(program, 'u_ripple_pos');
  const uRippleT = gl.getUniformLocation(program, 'u_ripple_t');

  const mx = new Spring(0.5, SPRING_GENTLE);
  const my = new Spring(0.75, SPRING_GENTLE);
  const hoverSpring = new Spring(0, SPRING_GENTLE);

  let rippleX = 0.5;
  let rippleY = 0.5;
  let rippleT = 5.0;
  let visible = true;
  let contextLost = false;
  let rafId = 0;
  let startTime = performance.now();
  let lastTime = startTime;

  const resize = () => {
    const r = hero.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 1.75);
    canvas.width = Math.max(1, Math.round(r.width * dpr));
    canvas.height = Math.max(1, Math.round(r.height * dpr));
    gl.viewport(0, 0, canvas.width, canvas.height);
  };
  resize();
  window.addEventListener('resize', resize);

  canvas.addEventListener('webglcontextlost', (e) => {
    e.preventDefault();
    contextLost = true;
  });

  hero.addEventListener('pointermove', (e) => {
    const r = hero.getBoundingClientRect();
    mx.target = (e.clientX - r.left) / Math.max(1, r.width);
    my.target = 1.0 - (e.clientY - r.top) / Math.max(1, r.height);
    hoverSpring.target = 1;
  });

  hero.addEventListener('pointerleave', () => {
    hoverSpring.target = 0;
  });

  hero.addEventListener('pointerdown', (e) => {
    if ((e.target as HTMLElement).closest('#playground')) return;
    const r = hero.getBoundingClientRect();
    rippleX = (e.clientX - r.left) / Math.max(1, r.width);
    rippleY = 1.0 - (e.clientY - r.top) / Math.max(1, r.height);
    rippleT = 0.0;
  });

  const tick = (now: number) => {
    if (!visible || contextLost) {
      rafId = 0;
      lastTime = 0;
      return;
    }
    const dt = lastTime ? Math.min((now - lastTime) / 1000, 0.05) : 1 / 60;
    lastTime = now;

    mx.step(dt);
    my.step(dt);
    hoverSpring.step(dt);
    if (rippleT < 2.0) rippleT += dt;

    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(uRes, canvas.width, canvas.height);
    gl.uniform2f(uMouse, mx.value, my.value);
    gl.uniform1f(uTime, (now - startTime) / 1000);
    gl.uniform1f(uHover, hoverSpring.value);
    gl.uniform2f(uRipplePos, rippleX, rippleY);
    gl.uniform1f(uRippleT, rippleT);
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    rafId = requestAnimationFrame(tick);
  };

  observeVisibility(hero, (isVis) => {
    visible = isVis;
    if (visible && !rafId && !contextLost) {
      lastTime = performance.now();
      rafId = requestAnimationFrame(tick);
    }
  });

  rafId = requestAnimationFrame(tick);
}
