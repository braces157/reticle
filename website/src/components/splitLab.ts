import { sound } from '../motion/audio';
import {
  observeVisibility,
  prefersReducedMotion,
  Spring,
  SPRING_ELASTIC,
} from '../motion/spring';
import { copyText } from './chrome';

interface SplitItem {
  title: string;
  leftMeta: string;
  rightMeta: string;
  leftHtml: string;
  rightCode: string;
}

type SplitKey = 'table' | 'code' | 'receipt';

const SPLIT_DATA: Record<SplitKey, SplitItem> = {
  table: {
    title: 'Annotated & Redacted',
    leftMeta: 'Raw Capture · Exposed API Key',
    rightMeta: 'Mosaic Redacted + Callout · Ready for Ctrl+C / Ctrl+S',
    leftHtml: `
      <div class="font-mono text-xs sm:text-sm leading-relaxed text-black/80 bg-white/75 p-4 rounded-lg border border-black/15 max-w-md">
        <div class="text-[10px] uppercase text-black/45 mb-2">.env.production — Unredacted</div>
        <div>PROVIDER=openai</div>
        <div class="text-red-700 font-semibold">API_KEY=sk-live-94f2a8c10b7d3e99</div>
        <div>ENDPOINT=https://api.internal/v2</div>
        <div class="mt-2 text-black/55">BUG: retry loop exits early on 429</div>
      </div>`,
    rightCode: `// Exported PNG / Clipboard RGBA (composite_annotations):\nPROVIDER=openai\nAPI_KEY=▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  <-- [Mosaic 12px]\nENDPOINT=https://api.internal/v2\n\n↗ Arrow callout -> "Fix 429 backoff before release"\nSaved: Pictures\\SnipTool\\Snip_20261123_001800.png`,
  },
  code: {
    title: 'Offline Dark-Mode OCR',
    leftMeta: 'Small Dark-Mode CJK UI · Low Contrast',
    rightMeta: 'Windows.Media.Ocr + Otsu Contrast Preprocessing',
    leftHtml: `
      <div class="bg-[#121216] text-white/75 p-4 rounded-lg border border-black/25 max-w-md space-y-1.5">
        <div class="text-[10px] font-mono text-white/35">Spotify / Dark App — 12px muted text</div>
        <div class="text-sm font-medium text-white/80">深夜の東京ドライブ · City Pop Mix</div>
        <div class="text-xs text-white/50">再生中: Plastic Love (Remastered 2024)</div>
      </div>`,
    rightCode: `// Extracted offline via Windows.Media.Ocr (0 bytes sent):\n深夜の東京ドライブ · City Pop Mix\n再生中: Plastic Love (Remastered 2024)\n\n→ Copied cleanly to clipboard (O)`,
  },
  receipt: {
    title: 'Instant Translation',
    leftMeta: 'Foreign App Dialog · Unselectable UI',
    rightMeta: 'Free Translate or BYOK Gemini / OpenAI',
    leftHtml: `
      <div class="bg-[#17171D] text-white p-4 rounded-lg border border-black/20 max-w-md font-sans">
        <div class="text-[10px] font-mono text-accent mb-1.5">SYSTEM DIALOG · JP</div>
        <div class="text-sm font-semibold mb-1">接続がタイムアウトしました</div>
        <div class="text-xs text-white/65">ネットワーク設定を確認してから、もう一度お試しください。</div>
      </div>`,
    rightCode: `Connection timed out.\nPlease check your network settings and try again.\n\n── Original OCR ──\n接続がタイムアウトしました\nネットワーク設定を確認してから、もう一度お試しください。`,
  },
};

interface Sparkle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  alpha: number;
  warm: boolean;
}

export function initSplitLab(): void {
  const container = document.getElementById('split-container');
  const leftPane = document.getElementById('split-left-pane');
  const handle = document.getElementById('split-handle');
  const sparkleCanvas = document.getElementById('split-sparkle-canvas') as HTMLCanvasElement | null;
  const sparkleCtx = sparkleCanvas?.getContext('2d') ?? null;
  const bendLine = document.getElementById('split-bend-line');
  const bendGlow = document.getElementById('split-bend-glow');
  if (!container || !leftPane || !handle) return;

  const pctSpring = new Spring(50, { stiffness: 300, damping: 28, precision: 0.02 });
  const bendSpring = new Spring(0, SPRING_ELASTIC);
  const reduced = prefersReducedMotion();
  const sparkles: Sparkle[] = [];

  let activeKey: SplitKey = 'table';
  let pointerY = 0.5;
  let dragging = false;
  let visible = false;
  let rafId = 0;
  let lastTime = 0;

  const resizeCanvas = () => {
    if (!sparkleCanvas) return;
    const r = container.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    sparkleCanvas.width = Math.max(1, r.width) * dpr;
    sparkleCanvas.height = Math.max(1, r.height) * dpr;
    sparkleCtx?.setTransform(dpr, 0, 0, dpr, 0, 0);
  };
  resizeCanvas();
  window.addEventListener('resize', resizeCanvas);

  const drawFrame = () => {
    const r = container.getBoundingClientRect();
    const w = r.width || 800;
    const h = r.height || 360;
    const x = (pctSpring.value / 100) * w;
    const cy = Math.max(40, Math.min(h - 40, pointerY * h));

    leftPane.style.clipPath = `inset(0 ${100 - pctSpring.value}% 0 0)`;
    handle.style.left = `${pctSpring.value}%`;
    handle.setAttribute('aria-valuenow', String(Math.round(pctSpring.value)));

    const pathD = `M ${x.toFixed(1)},0 Q ${(x + bendSpring.value).toFixed(1)},${cy.toFixed(1)} ${x.toFixed(1)},${h.toFixed(1)}`;
    bendLine?.setAttribute('d', pathD);
    bendGlow?.setAttribute('d', pathD);

    if (sparkleCtx) {
      sparkleCtx.clearRect(0, 0, w, h);
      for (let i = sparkles.length - 1; i >= 0; i--) {
        const p = sparkles[i];
        p.x += p.vx;
        p.y += p.vy;
        p.vx *= 0.95;
        p.alpha *= 0.91;
        sparkleCtx.fillStyle = p.warm
          ? `rgba(255, 110, 82, ${p.alpha})`
          : `rgba(255, 255, 255, ${p.alpha})`;
        sparkleCtx.beginPath();
        sparkleCtx.arc(p.x, p.y, p.size, 0, Math.PI * 2);
        sparkleCtx.fill();
        if (p.alpha < 0.03) sparkles.splice(i, 1);
      }
    }
  };

  const tick = (now: number) => {
    if (!visible) {
      rafId = 0;
      lastTime = 0;
      return;
    }
    const dt = lastTime ? Math.min((now - lastTime) / 1000, 0.05) : 1 / 60;
    lastTime = now;

    const prev = pctSpring.value;
    const movingPct = pctSpring.step(dt);
    const velPerSec = dt > 0 ? (pctSpring.value - prev) / dt : 0;

    bendSpring.target = reduced ? 0 : Math.max(-26, Math.min(26, -velPerSec * 0.14));
    const movingBend = bendSpring.step(dt);

    if (!reduced && Math.abs(velPerSec) > 12 && sparkles.length < 44) {
      const r = container.getBoundingClientRect();
      const x = (pctSpring.value / 100) * (r.width || 800);
      const h = r.height || 360;
      const count = Math.min(3, Math.ceil(Math.abs(velPerSec) / 40));
      for (let i = 0; i < count; i++) {
        sparkles.push({
          x: x + (Math.random() - 0.5) * 4,
          y: Math.random() * h,
          vx: -Math.sign(velPerSec) * (0.9 + Math.random() * 2.2),
          vy: (Math.random() - 0.5) * 1.1,
          size: 1 + Math.random() * 1.7,
          alpha: 0.72 + Math.random() * 0.25,
          warm: Math.random() > 0.45,
        });
      }
    }

    drawFrame();

    if (movingPct || movingBend || sparkles.length > 0) {
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

  const setPercent = (pct: number) => {
    const clamped = Math.max(8, Math.min(92, pct));
    if (reduced) {
      pctSpring.snap(clamped);
      bendSpring.snap(0);
      drawFrame();
      return;
    }
    pctSpring.target = clamped;
    kick();
  };

  observeVisibility(container, (isVis) => {
    visible = isVis;
    if (visible) {
      resizeCanvas();
      drawFrame();
    }
  });

  const renderSplit = (key: SplitKey) => {
    activeKey = key;
    const item = SPLIT_DATA[key];
    const leftVisual = document.getElementById('split-left-visual');
    const rightCode = document.getElementById('split-right-code');
    const leftMeta = document.getElementById('split-left-meta');
    const rightMeta = document.getElementById('split-right-meta');
    const dynLabel = document.getElementById('split-dynamic-label');
    if (leftVisual) leftVisual.innerHTML = item.leftHtml;
    if (rightCode) rightCode.textContent = item.rightCode;
    if (leftMeta) leftMeta.textContent = item.leftMeta;
    if (rightMeta) rightMeta.textContent = item.rightMeta;
    if (dynLabel) dynLabel.textContent = item.title;
  };

  document.querySelectorAll<HTMLButtonElement>('.split-mode-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const key = btn.dataset.split as SplitKey | undefined;
      if (!key || !(key in SPLIT_DATA)) return;
      sound.play('tick');
      renderSplit(key);
    });
  });

  container.addEventListener('pointerdown', (e) => {
    if ((e.target as HTMLElement).closest('#split-copy-btn')) return;
    dragging = true;
    handle.classList.add('is-dragging');
    const r = container.getBoundingClientRect();
    pointerY = (e.clientY - r.top) / Math.max(1, r.height);
    setPercent(((e.clientX - r.left) / Math.max(1, r.width)) * 100);
  });

  window.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    const r = container.getBoundingClientRect();
    pointerY = Math.max(0.1, Math.min(0.9, (e.clientY - r.top) / Math.max(1, r.height)));
    setPercent(((e.clientX - r.left) / Math.max(1, r.width)) * 100);
  });

  window.addEventListener('pointerup', () => {
    dragging = false;
    handle.classList.remove('is-dragging');
  });

  handle.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowLeft') setPercent(pctSpring.target - 6);
    if (e.key === 'ArrowRight') setPercent(pctSpring.target + 6);
  });

  document.getElementById('split-copy-btn')?.addEventListener('click', () => {
    copyText(SPLIT_DATA[activeKey].rightCode, 'Copied structured output');
  });

  renderSplit('table');
  drawFrame();
}
