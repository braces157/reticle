import { sound } from '../motion/audio';
import { copyText, showToast } from './chrome';

interface RectBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

type HandleDir = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

export interface LiveSnipController {
  open: () => void;
  close: () => void;
  isOpen: () => boolean;
}

let pinCounter = 0;

/**
 * Full-viewport "Live Page Snip" overlay — lets visitors press `S` (or click Try Live Snip)
 * to literally snip any text/section of the landing page itself, OCR-extract real DOM text
 * inside the drawn box, or pin a floating draggable/zoomable/opacity card over the site!
 */
export function initLiveSnip(): LiveSnipController {
  const overlay = document.createElement('div');
  overlay.id = 'live-snip-overlay';
  overlay.className = 'live-snip-overlay hidden';
  overlay.setAttribute('role', 'dialog');
  overlay.setAttribute('aria-label', 'Live Page Snip Overlay');
  overlay.innerHTML = `
    <div class="live-snip-guide-h"></div>
    <div class="live-snip-guide-v"></div>
    <div class="live-snip-banner glass-panel">
      <span class="w-1.5 h-1.5 rounded-full bg-accent"></span>
      <span>Live Reticle Mode — drag over any text on this page</span>
      <span class="text-white/35">·</span>
      <kbd class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-white/10 text-white">Esc</kbd>
      <span class="text-white/50">exit</span>
    </div>
    <div class="live-snip-loupe font-mono text-[10px]">
      <span class="live-snip-coords">000, 000</span>
      <span class="text-white/30">·</span>
      <span class="text-accent">RETICLE</span>
    </div>
    <div class="live-snip-box reticle-box hidden">
      <span class="sel-handle" data-handle="nw"></span>
      <span class="sel-handle" data-handle="n"></span>
      <span class="sel-handle" data-handle="ne"></span>
      <span class="sel-handle" data-handle="e"></span>
      <span class="sel-handle" data-handle="se"></span>
      <span class="sel-handle" data-handle="s"></span>
      <span class="sel-handle" data-handle="sw"></span>
      <span class="sel-handle" data-handle="w"></span>
      <div class="live-snip-dims">0 × 0 px</div>
      <div class="live-snip-bar glass-panel hidden">
        <button type="button" data-action="pin" class="btn-glass px-2.5 py-1 rounded-lg text-[11px] font-medium">Pin (P)</button>
        <button type="button" data-action="ocr" class="btn-luminous px-2.5 py-1 rounded-lg text-[11px] font-semibold">OCR Copy (O)</button>
        <button type="button" data-action="cancel" aria-label="Exit live snip" class="w-6 h-6 inline-flex items-center justify-center rounded-lg text-white/50 hover:text-white hover:bg-white/10">✕</button>
      </div>
    </div>
  `;
  document.body.appendChild(overlay);

  const guideH = overlay.querySelector<HTMLElement>('.live-snip-guide-h')!;
  const guideV = overlay.querySelector<HTMLElement>('.live-snip-guide-v')!;
  const loupe = overlay.querySelector<HTMLElement>('.live-snip-loupe')!;
  const coordsEl = overlay.querySelector<HTMLElement>('.live-snip-coords')!;
  const selBox = overlay.querySelector<HTMLElement>('.live-snip-box')!;
  const dimsEl = overlay.querySelector<HTMLElement>('.live-snip-dims')!;
  const bar = overlay.querySelector<HTMLElement>('.live-snip-bar')!;

  let active = false;
  let box: RectBox = { x: 0, y: 0, w: 0, h: 0 };
  let mode:
    | { kind: 'idle' }
    | { kind: 'drag'; sx: number; sy: number }
    | { kind: 'move'; ox: number; oy: number }
    | { kind: 'resize'; handle: HandleDir; sx: number; sy: number; orig: RectBox } = { kind: 'idle' };

  const applyBox = () => {
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    box.w = Math.max(24, Math.min(vw, box.w));
    box.h = Math.max(20, Math.min(vh, box.h));
    box.x = Math.max(0, Math.min(vw - box.w, box.x));
    box.y = Math.max(0, Math.min(vh - box.h, box.y));

    selBox.style.left = `${box.x}px`;
    selBox.style.top = `${box.y}px`;
    selBox.style.width = `${box.w}px`;
    selBox.style.height = `${box.h}px`;
    dimsEl.textContent = `${Math.round(box.w)} × ${Math.round(box.h)} px`;

    const nearBottom = box.y + box.h + 52 > vh;
    bar.style.top = nearBottom ? '-44px' : 'auto';
    bar.style.bottom = nearBottom ? 'auto' : '-44px';
  };

  const open = () => {
    if (active) return;
    active = true;
    mode = { kind: 'idle' };
    selBox.classList.add('hidden');
    bar.classList.add('hidden');
    overlay.classList.remove('hidden');
    requestAnimationFrame(() => overlay.classList.add('is-active'));
    sound.play('shutter');
  };

  const close = () => {
    if (!active) return;
    active = false;
    mode = { kind: 'idle' };
    overlay.classList.remove('is-active');
    window.setTimeout(() => {
      if (!active) overlay.classList.add('hidden');
    }, 170);
  };

  const finishAction = (action: 'ocr' | 'pin') => {
    const extracted = extractVisibleTextInRect(box);
    const content = extracted || `Captured ${Math.round(box.w)} × ${Math.round(box.h)} px region`;
    if (action === 'ocr') {
      sound.play('scan');
      copyText(
        content,
        extracted ? `OCR copied ${extracted.length} chars from page` : 'Copied region telemetry',
      );
      close();
    } else {
      sound.play('pin');
      spawnFloatingPagePin(box, content);
      showToast('Pinned live page snip · Drag / Scroll zoom / Shift+Scroll opacity');
      close();
    }
  };

  bar.addEventListener('click', (e) => {
    e.stopPropagation();
    const btn = (e.target as HTMLElement).closest<HTMLButtonElement>('button[data-action]');
    const action = btn?.dataset.action;
    if (action === 'ocr') finishAction('ocr');
    else if (action === 'pin') finishAction('pin');
    else if (action === 'cancel') close();
  });

  overlay.addEventListener('pointerdown', (e) => {
    if (e.button === 2) {
      close();
      return;
    }
    const target = e.target as HTMLElement;
    if (target.closest('.live-snip-bar')) return;

    const handleEl = target.closest<HTMLElement>('.sel-handle');
    if (handleEl && !selBox.classList.contains('hidden')) {
      mode = {
        kind: 'resize',
        handle: handleEl.dataset.handle as HandleDir,
        sx: e.clientX,
        sy: e.clientY,
        orig: { ...box },
      };
      bar.classList.add('hidden');
      return;
    }

    const hasBox = !selBox.classList.contains('hidden');
    const inside =
      hasBox &&
      e.clientX >= box.x &&
      e.clientX <= box.x + box.w &&
      e.clientY >= box.y &&
      e.clientY <= box.y + box.h;

    if (inside) {
      mode = { kind: 'move', ox: e.clientX - box.x, oy: e.clientY - box.y };
      bar.classList.add('hidden');
      return;
    }

    mode = { kind: 'drag', sx: e.clientX, sy: e.clientY };
    box = { x: e.clientX, y: e.clientY, w: 0, h: 0 };
    selBox.classList.remove('hidden');
    bar.classList.add('hidden');
    selBox.style.left = `${box.x}px`;
    selBox.style.top = `${box.y}px`;
    selBox.style.width = '0px';
    selBox.style.height = '0px';
  });

  overlay.addEventListener('pointermove', (e) => {
    guideH.style.top = `${e.clientY}px`;
    guideV.style.left = `${e.clientX}px`;
    const lx = e.clientX + 16 + 150 > window.innerWidth ? e.clientX - 156 : e.clientX + 16;
    const ly = e.clientY + 16 + 34 > window.innerHeight ? e.clientY - 36 : e.clientY + 16;
    loupe.style.transform = `translate3d(${lx}px, ${ly}px, 0)`;
    coordsEl.textContent = `${String(Math.round(e.clientX)).padStart(3, '0')}, ${String(Math.round(e.clientY)).padStart(3, '0')}`;

    if (mode.kind === 'drag') {
      box = {
        x: Math.min(mode.sx, e.clientX),
        y: Math.min(mode.sy, e.clientY),
        w: Math.abs(e.clientX - mode.sx),
        h: Math.abs(e.clientY - mode.sy),
      };
      selBox.style.left = `${box.x}px`;
      selBox.style.top = `${box.y}px`;
      selBox.style.width = `${box.w}px`;
      selBox.style.height = `${box.h}px`;
      dimsEl.textContent = `${Math.round(box.w)} × ${Math.round(box.h)} px`;
    } else if (mode.kind === 'move') {
      box.x = e.clientX - mode.ox;
      box.y = e.clientY - mode.oy;
      applyBox();
    } else if (mode.kind === 'resize') {
      box = resizeLiveBox(mode.orig, mode.handle, e.clientX - mode.sx, e.clientY - mode.sy);
      applyBox();
    }
  });

  window.addEventListener('pointerup', () => {
    if (!active || mode.kind === 'idle') return;
    const prev = mode.kind;
    mode = { kind: 'idle' };

    if (prev === 'drag' && (box.w < 16 || box.h < 16)) {
      selBox.classList.add('hidden');
      bar.classList.add('hidden');
      return;
    }

    applyBox();
    bar.classList.remove('hidden');
    if (prev === 'drag') sound.play('tick');
  });

  overlay.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    close();
  });

  window.addEventListener('keydown', (e) => {
    const target = e.target as HTMLElement | null;
    if (target?.matches('input, textarea')) return;

    if (active) {
      if (e.key === 'Escape') {
        e.preventDefault();
        close();
      } else if (!selBox.classList.contains('hidden')) {
        if (e.key === 'Enter' || e.key.toLowerCase() === 'o') {
          e.preventDefault();
          finishAction('ocr');
        } else if (e.key.toLowerCase() === 'p') {
          e.preventDefault();
          finishAction('pin');
        }
      }
      return;
    }

    // Pressing `S` (no modifiers) anywhere on the site launches Live Page Snip!
    if (e.key.toLowerCase() === 's' && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const anyModalOpen = document.querySelector('.modal-backdrop.is-open, .cmdk-backdrop.is-open');
      if (!anyModalOpen) {
        e.preventDefault();
        open();
      }
    }
  });

  document.getElementById('hero-live-snip-btn')?.addEventListener('click', open);

  return { open, close, isOpen: () => active };
}

function resizeLiveBox(orig: RectBox, handle: HandleDir, dx: number, dy: number): RectBox {
  const minW = 48;
  const minH = 36;
  let { x, y, w, h } = orig;
  if (handle.includes('e')) w = Math.max(minW, orig.w + dx);
  if (handle.includes('s')) h = Math.max(minH, orig.h + dy);
  if (handle.includes('w')) {
    const clamped = Math.min(dx, orig.w - minW);
    x = orig.x + clamped;
    w = orig.w - clamped;
  }
  if (handle.includes('n')) {
    const clamped = Math.min(dy, orig.h - minH);
    y = orig.y + clamped;
    h = orig.h - clamped;
  }
  return { x, y, w, h };
}

function extractVisibleTextInRect(rect: RectBox): string {
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const chunks: string[] = [];
  const seen = new Set<string>();
  const rRight = rect.x + rect.w;
  const rBottom = rect.y + rect.h;

  let node = walker.nextNode();
  while (node) {
    const raw = node.textContent?.replace(/\s+/g, ' ').trim();
    const parent = node.parentElement;
    if (
      raw &&
      parent &&
      !parent.closest('#live-snip-overlay, #toast, .live-floating-pin, .modal-backdrop, .cmdk-backdrop, script, style')
    ) {
      const range = document.createRange();
      range.selectNodeContents(node);
      const b = range.getBoundingClientRect();
      const overlaps =
        b.width > 0 &&
        b.height > 0 &&
        b.right >= rect.x &&
        b.left <= rRight &&
        b.bottom >= rect.y &&
        b.top <= rBottom;
      if (overlaps && !seen.has(raw)) {
        seen.add(raw);
        chunks.push(raw);
      }
    }
    node = walker.nextNode();
  }
  return chunks.join('\n').slice(0, 600);
}

function spawnFloatingPagePin(box: RectBox, content: string): void {
  pinCounter += 1;
  const card = document.createElement('div');
  card.className = 'live-floating-pin glass-panel';
  card.style.left = `${Math.max(12, Math.min(window.innerWidth - 290, box.x))}px`;
  card.style.top = `${Math.max(64, Math.min(window.innerHeight - 190, box.y))}px`;
  card.innerHTML = `
    <div class="flex items-center justify-between gap-2 mb-1.5 text-[10px] font-mono">
      <span class="text-accent font-semibold">PIN #${pinCounter} · <span class="pin-zoom-label text-white/70">100%</span></span>
      <div class="flex items-center gap-1.5">
        <button type="button" class="pin-copy-btn text-white/60 hover:text-white px-1">Copy</button>
        <button type="button" class="pin-close-btn text-white/55 hover:text-white px-1" aria-label="Close pinned snip">✕</button>
      </div>
    </div>
    <div class="pin-body font-mono text-[11px] text-white/90 whitespace-pre-wrap leading-relaxed max-h-36 overflow-y-auto thin-scroll"></div>
    <div class="mt-2 pt-1.5 border-t border-white/10 flex items-center justify-between font-mono text-[9.5px] text-white/40">
      <span>Drag · Scroll zoom</span>
      <span>Shift+Scroll opacity</span>
    </div>
  `;
  card.querySelector<HTMLElement>('.pin-body')!.textContent = content;
  document.body.appendChild(card);

  let zoom = 1;
  let opacity = 1;
  let dragging = false;
  let ox = 0;
  let oy = 0;
  const zoomLabel = card.querySelector<HTMLElement>('.pin-zoom-label')!;

  card.querySelector('.pin-copy-btn')?.addEventListener('click', (e) => {
    e.stopPropagation();
    copyText(content, 'Copied pinned snip');
  });
  card.querySelector('.pin-close-btn')?.addEventListener('click', (e) => {
    e.stopPropagation();
    card.remove();
  });
  card.addEventListener('dblclick', () => card.remove());

  card.addEventListener('pointerdown', (e) => {
    if ((e.target as HTMLElement).closest('button')) return;
    dragging = true;
    const r = card.getBoundingClientRect();
    ox = e.clientX - r.left;
    oy = e.clientY - r.top;
    card.setPointerCapture(e.pointerId);
  });
  card.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    card.style.left = `${Math.max(8, Math.min(window.innerWidth - card.offsetWidth - 8, e.clientX - ox))}px`;
    card.style.top = `${Math.max(8, Math.min(window.innerHeight - card.offsetHeight - 8, e.clientY - oy))}px`;
  });
  card.addEventListener('pointerup', () => {
    dragging = false;
  });

  card.addEventListener(
    'wheel',
    (e) => {
      e.preventDefault();
      if (e.shiftKey) {
        opacity = Math.max(0.25, Math.min(1, opacity + (e.deltaY < 0 ? 0.08 : -0.08)));
        card.style.opacity = opacity.toFixed(2);
        zoomLabel.textContent = `α ${Math.round(opacity * 100)}%`;
      } else {
        zoom = Math.max(0.75, Math.min(1.45, zoom + (e.deltaY < 0 ? 0.08 : -0.08)));
        card.style.transform = `scale(${zoom.toFixed(2)})`;
        zoomLabel.textContent = `${Math.round(zoom * 100)}%`;
      }
    },
    { passive: false },
  );
}
