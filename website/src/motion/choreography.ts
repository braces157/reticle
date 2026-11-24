import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { prefersReducedMotion, Spring, SPRING_GENTLE } from './spring';

gsap.registerPlugin(ScrollTrigger);

export function initChoreography(): void {
  initScrollLaser();
  initCursorSpeculars();

  if (prefersReducedMotion()) {
    document.querySelectorAll<HTMLElement>('.bench-bar').forEach((bar) => {
      bar.style.width = bar.dataset.width ?? '50%';
    });
    return;
  }

  initMagneticButtons();
  initHeroEntrance();
  initHeroMicroParallax();
  initScrollReveals();
}

function initScrollLaser(): void {
  const laser = document.getElementById('scroll-laser');
  if (!laser) return;
  const update = () => {
    const docH = document.documentElement.scrollHeight - window.innerHeight;
    laser.style.width = `${docH > 0 ? (window.scrollY / docH) * 100 : 0}%`;
  };
  update();
  window.addEventListener('scroll', update, { passive: true });
}

function initCursorSpeculars(): void {
  document.querySelectorAll<HTMLElement>('.spotlight-card').forEach((card) => {
    card.addEventListener('pointermove', (e) => {
      const r = card.getBoundingClientRect();
      card.style.setProperty('--mx', `${e.clientX - r.left}px`);
      card.style.setProperty('--my', `${e.clientY - r.top}px`);
    });
  });

  document.querySelectorAll<HTMLElement>('.btn-glass').forEach((btn) => {
    btn.addEventListener('pointermove', (e) => {
      const r = btn.getBoundingClientRect();
      btn.style.setProperty('--bx', `${e.clientX - r.left}px`);
      btn.style.setProperty('--by', `${e.clientY - r.top}px`);
    });
  });
}

function initMagneticButtons(): void {
  document.querySelectorAll<HTMLElement>('.magnetic-btn').forEach((btn) => {
    const xTo = gsap.quickTo(btn, 'x', { duration: 0.32, ease: 'power3.out' });
    const yTo = gsap.quickTo(btn, 'y', { duration: 0.32, ease: 'power3.out' });
    btn.addEventListener('pointermove', (e) => {
      const r = btn.getBoundingClientRect();
      xTo((e.clientX - (r.left + r.width / 2)) * 0.14);
      yTo((e.clientY - (r.top + r.height / 2)) * 0.14);
    });
    btn.addEventListener('pointerleave', () => {
      xTo(0);
      yTo(0);
    });
  });
}

function initHeroEntrance(): void {
  gsap.from('.hero-blur-in', {
    y: 18,
    opacity: 0,
    filter: 'blur(8px)',
    duration: 0.78,
    stagger: 0.06,
    ease: 'power3.out',
    clearProps: 'transform,opacity,filter',
  });

  gsap.fromTo(
    '#hero-display-3d',
    { rotateX: 5.5, scale: 0.965, y: 22, opacity: 0 },
    {
      rotateX: 1.8,
      scale: 1,
      y: 0,
      opacity: 1,
      duration: 0.95,
      delay: 0.16,
      ease: 'expo.out',
    },
  );

  gsap.to('#hero-display-3d', {
    rotateX: 0,
    ease: 'none',
    scrollTrigger: {
      trigger: '#playground',
      start: 'top 80%',
      end: 'top 22%',
      scrub: 0.5,
    },
  });
}

function initHeroMicroParallax(): void {
  const wrap = document.getElementById('playground');
  const display = document.getElementById('hero-display-3d');
  if (!wrap || !display) return;

  const rotY = new Spring(0, SPRING_GENTLE);
  let rafId = 0;
  let lastTime = 0;

  const tick = (now: number) => {
    const dt = lastTime ? (now - lastTime) / 1000 : 1 / 60;
    lastTime = now;
    const active = rotY.step(dt);
    display.style.setProperty('--tilt-y', `${rotY.value.toFixed(3)}deg`);
    if (active) {
      rafId = requestAnimationFrame(tick);
    } else {
      rafId = 0;
      lastTime = 0;
    }
  };

  const kick = () => {
    if (!rafId) {
      lastTime = performance.now();
      rafId = requestAnimationFrame(tick);
    }
  };

  wrap.addEventListener('pointermove', (e) => {
    if (e.buttons !== 0) {
      rotY.target = 0;
      kick();
      return;
    }
    const r = wrap.getBoundingClientRect();
    const nx = ((e.clientX - r.left) / Math.max(1, r.width)) * 2 - 1;
    rotY.target = nx * 1.1;
    kick();
  });

  wrap.addEventListener('pointerleave', () => {
    rotY.target = 0;
    kick();
  });
}

function initScrollReveals(): void {
  document.querySelectorAll<HTMLElement>('.scroll-reveal').forEach((el) => {
    gsap.from(el, {
      y: 22,
      opacity: 0,
      scale: 0.985,
      duration: 0.72,
      ease: 'power3.out',
      clearProps: 'transform,opacity',
      scrollTrigger: { trigger: el, start: 'top 90%', once: true },
    });
  });

  document.querySelectorAll<HTMLElement>('.bench-bar').forEach((bar) => {
    gsap.fromTo(
      bar,
      { width: '0%' },
      {
        width: bar.dataset.width ?? '50%',
        duration: 0.9,
        ease: 'expo.out',
        scrollTrigger: { trigger: bar, start: 'top 90%', once: true },
      },
    );
  });
}
