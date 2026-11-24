import { sound } from '../motion/audio';
import { prefersReducedMotion, Spring, SPRING_SNAPPY } from '../motion/spring';

let toastTimer: number | null = null;

export function showToast(msg: string): void {
  const toast = document.getElementById('toast');
  const label = document.getElementById('toast-msg');
  if (!toast || !label) return;
  label.textContent = msg;
  toast.classList.add('is-visible');
  if (toastTimer !== null) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toast.classList.remove('is-visible');
  }, 2200);
}

export function copyText(text: string, label?: string): void {
  const done = () => {
    sound.play('tick');
    showToast(label ?? 'Copied to clipboard');
  };
  if (navigator.clipboard?.writeText) {
    navigator.clipboard.writeText(text).then(done).catch(() => fallbackCopy(text, done));
  } else {
    fallbackCopy(text, done);
  }
}

function fallbackCopy(text: string, cb: () => void): void {
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand('copy');
  } catch {
    // Ignore clipboard fallback errors.
  }
  document.body.removeChild(ta);
  cb();
}

export function initChrome(): void {
  initClock();
  initSfxToggle();
  initMobileMenu();
  initModals();
  initSpringSegTracks();
}

function initClock(): void {
  const el = document.getElementById('live-clock');
  if (!el) return;
  const tick = () => {
    const d = new Date();
    el.textContent = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  };
  tick();
  window.setInterval(tick, 30_000);
}

function initSfxToggle(): void {
  const btn = document.getElementById('sfx-toggle');
  const label = document.getElementById('sfx-label');
  if (!btn || !label) return;
  btn.addEventListener('click', () => {
    const on = sound.toggle();
    label.textContent = on ? 'SFX' : 'MUTE';
  });
}

function initMobileMenu(): void {
  const btn = document.getElementById('mobile-menu-btn');
  const menu = document.getElementById('mobile-menu');
  if (!btn || !menu) return;
  btn.addEventListener('click', () => menu.classList.toggle('hidden'));
  document.querySelectorAll('.mobile-nav-link').forEach((link) => {
    link.addEventListener('click', () => menu.classList.add('hidden'));
  });
}

function initModals(): void {
  const dlModal = document.getElementById('download-modal');
  const shortcutsModal = document.getElementById('shortcuts-modal');
  const modalPlanTitle = document.getElementById('modal-plan-title');
  let lastTrigger: HTMLElement | null = null;

  const openModal = (modal: HTMLElement | null, trigger?: HTMLElement | null) => {
    if (!modal) return;
    lastTrigger = trigger ?? (document.activeElement as HTMLElement | null);
    sound.play('tick');
    modal.classList.add('is-open');
    modal.setAttribute('aria-hidden', 'false');
    const closeBtn = modal.querySelector<HTMLButtonElement>('button[aria-label^="Close"]');
    closeBtn?.focus();
  };

  const closeModal = (modal: HTMLElement | null) => {
    if (!modal || !modal.classList.contains('is-open')) return;
    modal.classList.remove('is-open');
    modal.setAttribute('aria-hidden', 'true');
    lastTrigger?.focus();
    lastTrigger = null;
  };

  const openDownload = (planTitle?: string, trigger?: HTMLElement | null) => {
    if (modalPlanTitle) {
      modalPlanTitle.textContent = planTitle
        ? `Reticle v0.1 — ${planTitle}`
        : 'Reticle v0.1 — Windows x64';
    }
    openModal(dlModal, trigger);
  };

  document.getElementById('nav-download-btn')?.addEventListener('click', (e) => {
    openDownload(undefined, e.currentTarget as HTMLElement);
  });
  document.getElementById('hero-download-btn')?.addEventListener('click', (e) => {
    openDownload(undefined, e.currentTarget as HTMLElement);
  });
  document.querySelectorAll<HTMLElement>('.open-dl-modal').forEach((btn) => {
    btn.addEventListener('click', () => openDownload(btn.dataset.plan, btn));
  });

  document.getElementById('close-dl-modal')?.addEventListener('click', () => closeModal(dlModal));
  dlModal?.addEventListener('click', (e) => {
    if (e.target === dlModal) closeModal(dlModal);
  });

  document.getElementById('modal-cli-copy')?.addEventListener('click', () => {
    copyText('cargo build --release', 'Copied "cargo build --release"');
  });
  document.getElementById('modal-start-dl')?.addEventListener('click', () => {
    copyText('.\\target\\release\\snip-translate.exe', 'Copied launch command');
    closeModal(dlModal);
  });

  const toggleShortcuts = (trigger?: HTMLElement | null) => {
    if (!shortcutsModal) return;
    if (shortcutsModal.classList.contains('is-open')) {
      closeModal(shortcutsModal);
    } else {
      openModal(shortcutsModal, trigger);
    }
  };

  document.getElementById('hero-shortcut-btn')?.addEventListener('click', (e) => {
    toggleShortcuts(e.currentTarget as HTMLElement);
  });
  document.getElementById('footer-shortcuts-btn')?.addEventListener('click', (e) => {
    toggleShortcuts(e.currentTarget as HTMLElement);
  });
  document
    .getElementById('close-shortcuts-modal')
    ?.addEventListener('click', () => closeModal(shortcutsModal));
  shortcutsModal?.addEventListener('click', (e) => {
    if (e.target === shortcutsModal) closeModal(shortcutsModal);
  });

  window.addEventListener('keydown', (e) => {
    const target = e.target as HTMLElement | null;
    if (e.key === '?' && !target?.matches('input, textarea')) {
      toggleShortcuts();
    } else if (e.key === 'Escape') {
      closeModal(dlModal);
      closeModal(shortcutsModal);
    }
  });
}

/**
 * 2D Spring-physics sliding pill indicator for all `.seg-track` segmented controls.
 * Tracks x, y, width, and height via ResizeObserver + post-layout RAF so dynamic
 * sibling reflows or narrow-screen wraps can never desync the active pill.
 */
function initSpringSegTracks(): void {
  const reduced = prefersReducedMotion();

  document.querySelectorAll<HTMLElement>('.seg-track').forEach((track) => {
    const indicator = document.createElement('span');
    indicator.className = 'seg-indicator';
    track.prepend(indicator);

    const xSpring = new Spring(0, SPRING_SNAPPY);
    const ySpring = new Spring(0, SPRING_SNAPPY);
    const wSpring = new Spring(0, SPRING_SNAPPY);
    const hSpring = new Spring(0, SPRING_SNAPPY);
    let rafId = 0;
    let lastTime = 0;

    const btns = Array.from(track.querySelectorAll<HTMLButtonElement>('button'));
    let activeBtn: HTMLButtonElement | undefined =
      btns.find((b) => b.classList.contains('bg-white')) ?? btns[0];

    const applyIndicator = () => {
      indicator.style.width = `${Math.max(0, wSpring.value).toFixed(2)}px`;
      indicator.style.height = `${Math.max(0, hSpring.value).toFixed(2)}px`;
      indicator.style.transform = `translate3d(${xSpring.value.toFixed(2)}px, ${ySpring.value.toFixed(2)}px, 0)`;
    };

    const tick = (now: number) => {
      const dt = lastTime ? (now - lastTime) / 1000 : 1 / 60;
      lastTime = now;
      const mx = xSpring.step(dt);
      const my = ySpring.step(dt);
      const mw = wSpring.step(dt);
      const mh = hSpring.step(dt);
      applyIndicator();
      if (mx || my || mw || mh) {
        rafId = requestAnimationFrame(tick);
      } else {
        rafId = 0;
        lastTime = 0;
      }
    };

    const setActiveStyles = (target: HTMLButtonElement) => {
      activeBtn = target;
      btns.forEach((x) => {
        const isAct = x === target;
        x.classList.remove('bg-white');
        if (isAct) {
          x.classList.remove('text-white/55', 'hover:text-white', 'font-medium');
          x.classList.add('text-black', 'font-semibold');
        } else {
          x.classList.remove('text-black', 'font-semibold');
          x.classList.add('text-white/55', 'hover:text-white', 'font-medium');
        }
      });
    };

    const moveTo = (btn: HTMLButtonElement, instant = false) => {
      const targetX = btn.offsetLeft;
      const targetY = btn.offsetTop;
      const targetW = btn.offsetWidth;
      const targetH = btn.offsetHeight;
      if (targetW === 0 || targetH === 0) return;

      if (instant || reduced) {
        xSpring.snap(targetX);
        ySpring.snap(targetY);
        wSpring.snap(targetW);
        hSpring.snap(targetH);
        applyIndicator();
        return;
      }
      xSpring.target = targetX;
      ySpring.target = targetY;
      wSpring.target = targetW;
      hSpring.target = targetH;
      if (!rafId) {
        lastTime = performance.now();
        rafId = requestAnimationFrame(tick);
      }
    };

    btns.forEach((b) => {
      b.classList.remove('bg-white');
      b.addEventListener('click', () => {
        setActiveStyles(b);
        // Measure on next frame AFTER sibling click handlers (e.g. dynamic heading text) settle
        requestAnimationFrame(() => moveTo(b, false));
      });
    });

    // External sync hook (e.g. when dragging on playground selects a preset programmatically)
    track.addEventListener('seg-select', (e) => {
      const btn = (e as CustomEvent<HTMLButtonElement>).detail;
      if (btn && btns.includes(btn)) {
        setActiveStyles(btn);
        requestAnimationFrame(() => moveTo(btn, false));
      }
    });

    if (activeBtn) {
      setActiveStyles(activeBtn);
      requestAnimationFrame(() => {
        if (activeBtn) moveTo(activeBtn, true);
      });
    }

    if (typeof ResizeObserver !== 'undefined') {
      const ro = new ResizeObserver(() => {
        if (activeBtn) moveTo(activeBtn, true);
      });
      ro.observe(track);
    }

    window.addEventListener('resize', () => {
      if (activeBtn) moveTo(activeBtn, true);
    });
  });
}
