import { sound } from '../motion/audio';
import { copyText } from './chrome';
import type { LiveSnipController } from './liveSnip';

interface CommandItem {
  id: string;
  label: string;
  hint: string;
  shortcut?: string;
  run: () => void;
}

export function initCommandPalette(liveSnip: LiveSnipController): void {
  const backdrop = document.createElement('div');
  backdrop.id = 'cmdk-modal';
  backdrop.className = 'cmdk-backdrop fixed inset-0 z-50 flex items-start justify-center pt-[14vh] p-4 bg-black/70 backdrop-blur-md';
  backdrop.setAttribute('role', 'dialog');
  backdrop.setAttribute('aria-modal', 'true');
  backdrop.setAttribute('aria-hidden', 'true');
  backdrop.setAttribute('aria-label', 'Reticle Command Palette');
  backdrop.innerHTML = `
    <div class="cmdk-dialog w-full max-w-lg rounded-2xl glass-panel overflow-hidden">
      <div class="px-4 py-3 border-b border-white/[0.08] flex items-center gap-3">
        <span class="font-mono text-xs text-accent">⌘</span>
        <input id="cmdk-input" type="text" placeholder="Type a command or jump to section…" autocomplete="off"
          class="w-full bg-transparent text-sm text-white placeholder:text-white/35 focus:outline-none" />
        <kbd class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-white/10 text-white/60">Esc</kbd>
      </div>
      <div id="cmdk-list" class="p-2 max-h-72 overflow-y-auto thin-scroll space-y-0.5"></div>
      <div class="px-4 py-2 border-t border-white/[0.06] bg-void/40 flex items-center justify-between font-mono text-[10px] text-white/40">
        <span>↑↓ navigate · Enter select</span>
        <span>Reticle Command Palette</span>
      </div>
    </div>
  `;
  document.body.appendChild(backdrop);

  const input = backdrop.querySelector<HTMLInputElement>('#cmdk-input')!;
  const list = backdrop.querySelector<HTMLElement>('#cmdk-list')!;

  const jumpTo = (selector: string) => {
    document.querySelector(selector)?.scrollIntoView({ behavior: 'smooth' });
  };

  const commands: CommandItem[] = [
    {
      id: 'live-snip',
      label: 'Try Live Page Snip Overlay',
      hint: 'Freeze this page, OCR text, or pin floating card',
      shortcut: 'S',
      run: () => liveSnip.open(),
    },
    {
      id: 'sim-capture',
      label: 'Simulate Local Snip + OCR Audit',
      hint: 'Trigger oscilloscope spike with 0 bytes egress',
      run: () => {
        jumpTo('#engine');
        window.setTimeout(() => document.getElementById('run-audit-btn')?.click(), 350);
      },
    },
    {
      id: 'copy-launch',
      label: 'Copy Launch Command (.exe)',
      hint: '.\\target\\release\\snip-translate.exe',
      run: () => copyText('.\\target\\release\\snip-translate.exe', 'Copied launch command'),
    },
    {
      id: 'copy-cargo',
      label: 'Copy Cargo Release Build Command',
      hint: 'cargo build --release',
      run: () => copyText('cargo build --release', 'Copied "cargo build --release"'),
    },
    {
      id: 'toggle-sfx',
      label: 'Toggle Optical Shutter Sound (SFX)',
      hint: 'Mechanical Leica shutter & keycap acoustics',
      run: () => document.getElementById('sfx-toggle')?.click(),
    },
    {
      id: 'shortcuts',
      label: 'Open Global Shortcuts Sheet',
      hint: 'View all Reticle hotkeys',
      shortcut: '?',
      run: () => document.getElementById('hero-shortcut-btn')?.click(),
    },
    {
      id: 'jump-playground',
      label: 'Jump to Interactive Playground',
      hint: '8 handles · live canvas drawing · pinned card',
      run: () => jumpTo('#playground'),
    },
    {
      id: 'jump-split',
      label: 'Jump to Elastic Compare Curtain',
      hint: 'Mosaic redaction · dark-mode OCR · translate',
      run: () => jumpTo('#split-lab'),
    },
    {
      id: 'jump-tools',
      label: 'Jump to 5-Card Bento Tools',
      hint: 'Annotations · Pin opacity · 5-color palette',
      run: () => jumpTo('#capabilities'),
    },
    {
      id: 'jump-pricing',
      label: 'Jump to Free Mode & BYOK AI',
      hint: '$0 forever · Gemini / OpenAI BYOK config',
      run: () => jumpTo('#pricing'),
    },
  ];

  let filtered = commands;
  let selectedIdx = 0;
  let lastFocused: HTMLElement | null = null;

  const isOpen = () => backdrop.classList.contains('is-open');

  const renderList = () => {
    const q = input.value.trim().toLowerCase();
    filtered = q
      ? commands.filter(
          (c) => c.label.toLowerCase().includes(q) || c.hint.toLowerCase().includes(q),
        )
      : commands;
    selectedIdx = Math.min(selectedIdx, Math.max(0, filtered.length - 1));

    if (filtered.length === 0) {
      list.innerHTML = `<div class="px-3 py-6 text-center font-mono text-xs text-white/40">No matching commands</div>`;
      return;
    }

    list.innerHTML = '';
    filtered.forEach((item, idx) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className =
        idx === selectedIdx
          ? 'w-full px-3 py-2.5 rounded-xl bg-white/[0.1] border border-white/15 flex items-center justify-between gap-3 text-left transition-colors'
          : 'w-full px-3 py-2.5 rounded-xl border border-transparent hover:bg-white/[0.05] flex items-center justify-between gap-3 text-left transition-colors';
      btn.innerHTML = `
        <div class="min-w-0">
          <div class="text-[13px] font-medium text-white truncate">${item.label}</div>
          <div class="text-[11px] text-white/45 truncate">${item.hint}</div>
        </div>
        ${item.shortcut ? `<kbd class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-white/10 text-white/80 shrink-0">${item.shortcut}</kbd>` : ''}
      `;
      btn.addEventListener('pointerenter', () => {
        selectedIdx = idx;
        renderList();
      });
      btn.addEventListener('click', () => execute(idx));
      list.appendChild(btn);
    });
  };

  const open = () => {
    if (isOpen()) return;
    lastFocused = document.activeElement as HTMLElement | null;
    sound.play('tick');
    input.value = '';
    selectedIdx = 0;
    renderList();
    backdrop.classList.add('is-open');
    backdrop.setAttribute('aria-hidden', 'false');
    requestAnimationFrame(() => input.focus());
  };

  const close = () => {
    if (!isOpen()) return;
    backdrop.classList.remove('is-open');
    backdrop.setAttribute('aria-hidden', 'true');
    lastFocused?.focus();
    lastFocused = null;
  };

  const execute = (idx: number) => {
    const item = filtered[idx];
    if (!item) return;
    close();
    sound.play('tick');
    item.run();
  };

  input.addEventListener('input', () => {
    selectedIdx = 0;
    renderList();
  });

  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIdx = (selectedIdx + 1) % Math.max(1, filtered.length);
      renderList();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIdx = (selectedIdx - 1 + Math.max(1, filtered.length)) % Math.max(1, filtered.length);
      renderList();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      execute(selectedIdx);
    }
  });

  backdrop.addEventListener('click', (e) => {
    if (e.target === backdrop) close();
  });

  document.getElementById('nav-cmdk-btn')?.addEventListener('click', open);

  window.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      if (isOpen()) close();
      else open();
    } else if (e.key === 'Escape' && isOpen()) {
      e.preventDefault();
      close();
    }
  });
}
