import { sound } from '../motion/audio';
import {
  observeVisibility,
  prefersReducedMotion,
  Spring,
  SPRING_SNAPPY,
} from '../motion/spring';
import { copyText } from './chrome';

/**
 * Spring-driven dual-layer optical X-Ray headline ("anything.").
 * Uses frame-rate-independent dt + IntersectionObserver pause when scrolled offscreen.
 */
export function initHeroScan(): void {
  const marquee = document.getElementById('hero-ocr-word');
  const statusEl = document.getElementById('hero-word-status');
  if (!marquee) return;

  const scanSpring = new Spring(68, SPRING_SNAPPY);
  const reduced = prefersReducedMotion();
  let hovering = false;
  let visible = true;
  let phase = 0;
  let rafId = 0;
  let lastTime = 0;

  const render = () => {
    marquee.style.setProperty('--scan-x', `${scanSpring.value.toFixed(2)}%`);
  };

  const tick = (now: number) => {
    if (!visible) {
      rafId = 0;
      lastTime = 0;
      return;
    }
    const dt = lastTime ? Math.min((now - lastTime) / 1000, 0.05) : 1 / 60;
    lastTime = now;

    if (!hovering && !reduced) {
      phase += dt * 1.15;
      scanSpring.target = 52 + Math.sin(phase) * 42;
    }

    const moving = scanSpring.step(dt);
    render();

    if ((!hovering && !reduced) || moving) {
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

  observeVisibility(marquee, (isVisible) => {
    visible = isVisible;
    if (visible && !reduced) kick();
  });

  marquee.addEventListener('pointerenter', () => {
    hovering = true;
  });

  marquee.addEventListener('pointermove', (e) => {
    const r = marquee.getBoundingClientRect();
    const pct = Math.max(4, Math.min(100, ((e.clientX - r.left) / Math.max(1, r.width)) * 100));
    scanSpring.target = pct;
    if (reduced) {
      scanSpring.snap(pct);
      render();
    } else {
      kick();
    }
  });

  marquee.addEventListener('pointerleave', () => {
    hovering = false;
    if (!reduced) kick();
  });

  marquee.addEventListener('click', () => {
    scanSpring.target = 100;
    kick();
    copyText('anything.', 'Copied "anything." to clipboard');
    sound.play('scan');
    if (statusEl) {
      statusEl.textContent = 'Copied ✓';
      statusEl.classList.add('text-emerald');
      window.setTimeout(() => {
        statusEl.textContent = 'Copy · Pin · Annotate';
        statusEl.classList.remove('text-emerald');
      }, 1500);
    }
  });

  render();
  if (!reduced) kick();
}
