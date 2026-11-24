import { sound } from '../motion/audio';
import { observeVisibility, prefersReducedMotion } from '../motion/spring';

/**
 * Visibility-gated 60fps local compute vs 0 KB/s outbound network oscilloscope.
 * Uses dt-based decay so 60Hz, 144Hz, and 240Hz displays decay at the exact same speed.
 */
export function initOscilloscope(): void {
  const canvas = document.getElementById('privacy-wave-canvas') as HTMLCanvasElement | null;
  const ctx = canvas?.getContext('2d') ?? null;
  const runBtn = document.getElementById('run-audit-btn');
  const msEl = document.getElementById('audit-ms');
  const countEl = document.getElementById('audit-count');
  const pulseEl = document.getElementById('audit-pulse-label');
  if (!canvas || !ctx) return;

  const samples = new Array<number>(80).fill(0);
  const reduced = prefersReducedMotion();
  let spike = 0;
  let auditCount = 1;
  let visible = false;
  let rafId = 0;
  let lastTime = 0;
  let accum = 0;

  const resize = () => {
    const r = canvas.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.max(200, r.width) * dpr;
    canvas.height = Math.max(40, r.height) * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  };
  resize();
  window.addEventListener('resize', resize);

  const draw = () => {
    const r = canvas.getBoundingClientRect();
    const w = r.width || 320;
    const h = r.height || 44;
    ctx.clearRect(0, 0, w, h);

    ctx.beginPath();
    for (let i = 0; i < samples.length; i++) {
      const x = (i / (samples.length - 1)) * w;
      const y = h - 8 - samples[i] * (h - 14);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.85)';
    ctx.lineWidth = 1.5;
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(0, h - 6);
    ctx.lineTo(w, h - 6);
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.24)';
    ctx.lineWidth = 1;
    ctx.setLineDash([3, 3]);
    ctx.stroke();
    ctx.setLineDash([]);
  };

  const tick = (now: number) => {
    if (!visible) {
      rafId = 0;
      lastTime = 0;
      return;
    }
    const dt = lastTime ? Math.min((now - lastTime) / 1000, 0.05) : 1 / 60;
    lastTime = now;

    accum += dt;
    while (accum >= 1 / 60) {
      samples.shift();
      samples.push(spike);
      spike *= 0.78;
      if (spike < 0.002) spike = 0;
      accum -= 1 / 60;
    }

    draw();

    const hasEnergy = spike > 0 || samples.some((v) => v > 0.003);
    if (hasEnergy && !reduced) {
      rafId = requestAnimationFrame(tick);
    } else {
      rafId = 0;
      lastTime = 0;
    }
  };

  const kick = () => {
    if (visible && !rafId) {
      lastTime = performance.now();
      rafId = requestAnimationFrame(tick);
    }
  };

  observeVisibility(canvas, (isVis) => {
    visible = isVis;
    if (visible) {
      resize();
      draw();
      if (spike > 0 || samples.some((v) => v > 0.003)) kick();
    }
  });

  runBtn?.addEventListener('click', () => {
    auditCount += 1;
    const ms = (14.2 + Math.random() * 4.6).toFixed(1);
    spike = 0.94;
    sound.play('scan');
    window.dispatchEvent(new CustomEvent('reticle:simulate-snip'));
    if (msEl) msEl.textContent = `${ms} ms`;
    if (countEl) countEl.textContent = String(auditCount);
    if (pulseEl) {
      pulseEl.textContent = `Local GDI + OCR · ${ms} ms`;
      pulseEl.classList.add('text-white/85');
      window.setTimeout(() => {
        pulseEl.textContent = 'Idle · Local Only';
        pulseEl.classList.remove('text-white/85');
      }, 1500);
    }
    if (reduced) {
      samples[samples.length - 4] = 0.85;
      draw();
    } else {
      kick();
    }
  });

  draw();
}
