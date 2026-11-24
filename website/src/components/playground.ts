import { sound } from '../motion/audio';
import { prefersReducedMotion } from '../motion/spring';
import { copyText, showToast } from './chrome';

type TargetKey = 'code' | 'invoice' | 'math' | 'tokyo';
type DrawTool = 'rect' | 'arrow' | 'mosaic' | null;
type HandleDir = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

interface BoxRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface DrawOp {
  tool: Exclude<DrawTool, null>;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

interface TargetPreset {
  mode: string;
  meta: string;
  formats: Record<string, string>;
}

const TARGETS: Record<TargetKey, TargetPreset> = {
  code: {
    mode: 'Ready to Copy / Annotate',
    meta: 'Drag handles to resize · Click □ / ↗ / Blur to draw live on canvas',
    formats: {
      'OCR Text (O)': `def extract_blocks(frame: np.ndarray) -> list[str]:\n    tensor = preprocess(frame, scale=2.0)\n    tokens = engine.infer(tensor, quantized=True)\n    return [t.text for t in tokens if t.conf > 0.94]`,
      'Image Export': `Clipboard: RGBA image copied via arboard (Ctrl+C / Enter)\nSaved PNG: %USERPROFILE%\\Pictures\\SnipTool\\Snip_20261123_001800.png\nAnnotations: composited cleanly into RGBA at export`,
    },
  },
  invoice: {
    mode: 'Pinned Floating Reference',
    meta: 'Click Pin to float card · Drag card & scroll-wheel to zoom',
    formats: {
      'OCR Text (O)': `Optical Hardware Procurement · INV-8841\nAspheric Glass Prism   12   $145.00   $1,740.00\nAnodized Focus Ring    08    $89.50     $716.00\nNet Due (USD)                        $2,456.00`,
      'Pin Controls': `Drag: Move floating window\nScroll: Zoom 25% – 400%\nShift+Scroll: Win32 layered opacity 20% – 100%\nRight-click: Copy · Save · OCR · Translate · Close`,
    },
  },
  math: {
    mode: 'AI Explain Code (BYOK)',
    meta: 'Press E · Gemini / OpenAI direct from local config.json',
    formats: {
      'Explain (E)': `apply_mosaic(img, block) redacts a rectangular region in-place:\n1. Averages RGB inside each block × block tile.\n2. Overwrites every pixel in that tile with the mean RGBA value.\n3. Runs on raw cropped pixels before PNG / clipboard export.`,
      'OCR Text (O)': `pub fn apply_mosaic(img: &mut CroppedImage, block: i32) {\n    let avg = sample_block_mean(&img.rgba, block);\n    fill_tile_rgba(&mut img.rgba, avg);\n}`,
    },
  },
  tokyo: {
    mode: 'Offline OCR + Translate',
    meta: 'Windows.Media.Ocr (Otsu Dark Mode) · JP → EN',
    formats: {
      'Translate (T)': `Optical Character Recognition\nBusiness Hours: 08:00 — 22:00\n1-14-7 Jinnan, Shibuya-ku, Tokyo`,
      'Original OCR (O)': `光学文字認識\n営業時間 08:00 — 22:00\n東京都渋谷区神南 1-14-7`,
    },
  },
};

export function initPlayground(): void {
  const stage = document.getElementById('snip-stage');
  const selBox = document.getElementById('selection-box');
  const selDims = document.getElementById('selection-dims');
  const selQuickbar = document.getElementById('selection-quickbar');
  const annotCanvas = document.getElementById('stage-annot-canvas') as HTMLCanvasElement | null;
  const annotCtx = annotCanvas?.getContext('2d') ?? null;
  const guideH = document.getElementById('guide-h');
  const guideV = document.getElementById('guide-v');
  const loupe = document.getElementById('cursor-loupe');
  const loupeCoords = document.getElementById('loupe-coords');
  const loupeHint = document.getElementById('loupe-hint');
  const hudMode = document.getElementById('hud-mode');
  const hudMeta = document.getElementById('hud-meta');
  const hudTabs = document.getElementById('hud-format-tabs');
  const hudOutput = document.getElementById('hud-output');
  const hudCopyBtn = document.getElementById('hud-copy-btn');
  const hudCopyLabel = document.getElementById('hud-copy-label');
  const pinnedCard = document.getElementById('pinned-stage-card');
  const pinnedText = document.getElementById('pinned-stage-text');
  const pinnedZoomBadge = document.getElementById('pinned-stage-zoom');

  if (!stage || !selBox || !selDims || !selQuickbar || !hudOutput || !hudTabs) return;

  let activeTarget: TargetKey = 'code';
  let activeFormat = 'OCR Text (O)';
  let box: BoxRect = { x: 24, y: 24, w: 320, h: 180 };
  let activeTool: DrawTool = null;
  let ops: DrawOp[] = [];
  let draftOp: DrawOp | null = null;
  let scrambleRaf = 0;

  type Interaction =
    | { kind: 'none' }
    | { kind: 'select'; startX: number; startY: number }
    | { kind: 'move'; offsetX: number; offsetY: number }
    | { kind: 'resize'; handle: HandleDir; startX: number; startY: number; orig: BoxRect }
    | { kind: 'draw'; tool: Exclude<DrawTool, null> };

  let interaction: Interaction = { kind: 'none' };

  const toolButtons: Record<Exclude<DrawTool, null>, HTMLElement | null> = {
    rect: document.getElementById('quick-rect-btn'),
    arrow: document.getElementById('quick-arrow-btn'),
    mosaic: document.getElementById('quick-mosaic-btn'),
  };
  const undoBtn = document.getElementById('quick-undo-btn') as HTMLButtonElement | null;

  const syncToolButtons = () => {
    (Object.keys(toolButtons) as Array<Exclude<DrawTool, null>>).forEach((k) => {
      toolButtons[k]?.classList.toggle('is-tool-active', activeTool === k);
    });
    if (undoBtn) {
      undoBtn.disabled = ops.length === 0;
      undoBtn.classList.toggle('opacity-35', ops.length === 0);
    }
  };

  const syncCanvasAndDraw = () => {
    if (!annotCanvas || !annotCtx) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const cw = Math.max(1, Math.round(box.w));
    const ch = Math.max(1, Math.round(box.h));
    if (annotCanvas.width !== cw * dpr || annotCanvas.height !== ch * dpr) {
      annotCanvas.width = cw * dpr;
      annotCanvas.height = ch * dpr;
    }
    annotCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
    annotCtx.clearRect(0, 0, cw, ch);

    const all = draftOp ? [...ops, draftOp] : ops;
    for (const op of all) {
      paintOp(annotCtx, op);
    }
  };

  const applyBoxRect = () => {
    const stageRect = stage.getBoundingClientRect();
    box.w = Math.max(48, Math.min(stageRect.width, box.w));
    box.h = Math.max(40, Math.min(stageRect.height, box.h));
    box.x = Math.max(0, Math.min(stageRect.width - box.w, box.x));
    box.y = Math.max(0, Math.min(stageRect.height - box.h, box.y));

    selBox.style.left = `${box.x}px`;
    selBox.style.top = `${box.y}px`;
    selBox.style.width = `${box.w}px`;
    selBox.style.height = `${box.h}px`;
    selDims.textContent = `${Math.round(box.w)} × ${Math.round(box.h)} px`;

    const nearBottom = box.y + box.h + 52 > stageRect.height;
    selQuickbar.style.bottom = nearBottom ? 'auto' : '-44px';
    selQuickbar.style.top = nearBottom ? '-44px' : 'auto';
    syncCanvasAndDraw();
  };

  const decodeTextAnimation = (targetStr: string) => {
    cancelAnimationFrame(scrambleRaf);
    if (prefersReducedMotion()) {
      hudOutput.textContent = targetStr;
      return;
    }
    const glyphs = '░▒▓01#*%+=';
    const len = targetStr.length;
    const start = performance.now();
    const duration = 260;

    const step = (now: number) => {
      const progress = Math.min(1, (now - start) / duration);
      const revealCount = Math.floor(progress * len);
      let out = '';
      for (let i = 0; i < len; i++) {
        const ch = targetStr[i];
        if (ch === '\n' || ch === ' ' || i < revealCount) out += ch;
        else out += glyphs[Math.floor(Math.random() * glyphs.length)];
      }
      hudOutput.textContent = out;
      if (progress < 1) scrambleRaf = requestAnimationFrame(step);
      else hudOutput.textContent = targetStr;
    };
    scrambleRaf = requestAnimationFrame(step);
  };

  const renderHud = (key: TargetKey, animate: boolean) => {
    const data = TARGETS[key];
    activeTarget = key;
    const formatKeys = Object.keys(data.formats);
    activeFormat = formatKeys[0];

    if (hudMode) hudMode.textContent = data.mode;
    if (hudMeta) hudMeta.textContent = data.meta;

    const presetTrack = document.getElementById('preset-seg-track');
    const activeBtn = presetTrack?.querySelector<HTMLButtonElement>(
      `.preset-snip-btn[data-target="${key}"]`,
    );
    if (presetTrack && activeBtn) {
      presetTrack.dispatchEvent(new CustomEvent('seg-select', { detail: activeBtn }));
    }

    hudTabs.innerHTML = '';
    formatKeys.forEach((fmt) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = fmt;
      b.className =
        fmt === activeFormat
          ? 'px-2.5 py-0.5 rounded-md bg-white/15 text-white font-medium'
          : 'px-2.5 py-0.5 rounded-md text-white/50 hover:text-white';
      b.addEventListener('click', () => {
        activeFormat = fmt;
        sound.play('tick');
        Array.from(hudTabs.children).forEach((child) => {
          child.className =
            child.textContent === fmt
              ? 'px-2.5 py-0.5 rounded-md bg-white/15 text-white font-medium'
              : 'px-2.5 py-0.5 rounded-md text-white/50 hover:text-white';
        });
        decodeTextAnimation(data.formats[fmt]);
      });
      hudTabs.appendChild(b);
    });

    const text = data.formats[activeFormat];
    if (animate) decodeTextAnimation(text);
    else hudOutput.textContent = text;
  };

  const fireLaser = () => {
    selBox.querySelector('.laser-line')?.remove();
    if (!prefersReducedMotion()) {
      const line = document.createElement('div');
      line.className = 'laser-line';
      selBox.appendChild(line);
      window.setTimeout(() => line.remove(), 480);
    }
    sound.play('scan');
  };

  const autoSnipTarget = (key: TargetKey) => {
    const el = document.getElementById(`target-${key}`);
    if (!el) return;
    const stageRect = stage.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();
    box = {
      x: elRect.left - stageRect.left + 8,
      y: elRect.top - stageRect.top + 8,
      w: Math.max(60, elRect.width - 16),
      h: Math.max(50, elRect.height - 16),
    };
    ops = [];
    draftOp = null;
    activeTool = null;
    syncToolButtons();
    selBox.classList.remove('hidden');
    selQuickbar.classList.remove('hidden');
    applyBoxRect();
    fireLaser();
    renderHud(key, true);
  };

  document.querySelectorAll<HTMLButtonElement>('.preset-snip-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const key = btn.dataset.target as TargetKey | undefined;
      if (key && key in TARGETS) autoSnipTarget(key);
    });
  });

  const toggleTool = (tool: Exclude<DrawTool, null>, label: string) => {
    sound.play('tick');
    if (activeTool === tool) {
      activeTool = null;
      showToast('Move / Resize mode');
    } else {
      activeTool = tool;
      if (!ops.some((o) => o.tool === tool)) {
        ops.push(makeStarterOp(tool, box));
        syncCanvasAndDraw();
      }
      showToast(`${label} active — drag inside selection to draw`);
    }
    syncToolButtons();
  };

  toolButtons.rect?.addEventListener('click', (e) => {
    e.stopPropagation();
    toggleTool('rect', 'Rectangle (1)');
  });
  toolButtons.arrow?.addEventListener('click', (e) => {
    e.stopPropagation();
    toggleTool('arrow', 'Arrow (2)');
  });
  toolButtons.mosaic?.addEventListener('click', (e) => {
    e.stopPropagation();
    toggleTool('mosaic', 'Mosaic Blur (5)');
  });
  undoBtn?.addEventListener('click', (e) => {
    e.stopPropagation();
    if (ops.length > 0) {
      ops.pop();
      sound.play('tick');
      syncCanvasAndDraw();
      syncToolButtons();
      showToast('Undid annotation (Ctrl+Z)');
    }
  });

  document.getElementById('quick-save-btn')?.addEventListener('click', (e) => {
    e.stopPropagation();
    sound.play('scan');
    showToast('Saved Pictures\\SnipTool\\Snip_20261123.png');
  });

  document.getElementById('quick-copy-btn')?.addEventListener('click', (e) => {
    e.stopPropagation();
    copyText(TARGETS[activeTarget].formats[activeFormat], `Copied ${activeFormat}`);
  });

  let pinZoom = 1;
  document.getElementById('quick-pin-btn')?.addEventListener('click', (e) => {
    e.stopPropagation();
    sound.play('tick');
    if (pinnedText) pinnedText.textContent = TARGETS[activeTarget].formats[activeFormat];
    if (pinnedCard) {
      pinZoom = 1;
      pinnedCard.style.transform = 'scale(1)';
      if (pinnedZoomBadge) pinnedZoomBadge.textContent = '100%';
      pinnedCard.classList.remove('hidden');
    }
    showToast('Pinned always-on-top — drag or scroll to zoom');
  });

  document.getElementById('close-pinned-stage')?.addEventListener('click', (e) => {
    e.stopPropagation();
    pinnedCard?.classList.add('hidden');
  });

  if (pinnedCard) {
    let draggingPin = false;
    let pinOffX = 0;
    let pinOffY = 0;

    pinnedCard.addEventListener('pointerdown', (e) => {
      if ((e.target as HTMLElement).closest('button')) return;
      e.stopPropagation();
      draggingPin = true;
      const r = pinnedCard.getBoundingClientRect();
      pinOffX = e.clientX - r.left;
      pinOffY = e.clientY - r.top;
      pinnedCard.setPointerCapture(e.pointerId);
    });
    pinnedCard.addEventListener('pointermove', (e) => {
      if (!draggingPin) return;
      const sr = stage.getBoundingClientRect();
      const nx = Math.max(8, Math.min(sr.width - pinnedCard.offsetWidth - 8, e.clientX - sr.left - pinOffX));
      const ny = Math.max(8, Math.min(sr.height - pinnedCard.offsetHeight - 8, e.clientY - sr.top - pinOffY));
      pinnedCard.style.left = `${nx}px`;
      pinnedCard.style.top = `${ny}px`;
    });
    pinnedCard.addEventListener('pointerup', () => {
      draggingPin = false;
    });
    pinnedCard.addEventListener(
      'wheel',
      (e) => {
        e.preventDefault();
        e.stopPropagation();
        pinZoom = Math.max(0.75, Math.min(1.35, pinZoom + (e.deltaY < 0 ? 0.08 : -0.08)));
        pinnedCard.style.transform = `scale(${pinZoom.toFixed(2)})`;
        if (pinnedZoomBadge) pinnedZoomBadge.textContent = `${Math.round(pinZoom * 100)}%`;
      },
      { passive: false },
    );
  }

  document.getElementById('quick-clear-btn')?.addEventListener('click', (e) => {
    e.stopPropagation();
    ops = [];
    draftOp = null;
    activeTool = null;
    syncToolButtons();
    selBox.classList.add('hidden');
    selQuickbar.classList.add('hidden');
  });

  const stageCoords = (e: PointerEvent) => {
    const r = stage.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(r.width, e.clientX - r.left)),
      y: Math.max(0, Math.min(r.height, e.clientY - r.top)),
      w: r.width,
      h: r.height,
    };
  };

  stage.addEventListener('pointerenter', () => {
    if (guideH) guideH.style.opacity = '1';
    if (guideV) guideV.style.opacity = '1';
    if (loupe) loupe.style.opacity = '1';
  });

  stage.addEventListener('pointerleave', () => {
    if (interaction.kind === 'none') {
      if (guideH) guideH.style.opacity = '0';
      if (guideV) guideV.style.opacity = '0';
      if (loupe) loupe.style.opacity = '0';
    }
  });

  stage.addEventListener('pointerdown', (e) => {
    const target = e.target as HTMLElement;
    if (
      target.closest('#ocr-hud') ||
      target.closest('#selection-quickbar') ||
      target.closest('#pinned-stage-card')
    ) {
      return;
    }

    const pt = stageCoords(e);

    const handleEl = target.closest<HTMLElement>('.sel-handle');
    if (handleEl && !selBox.classList.contains('hidden')) {
      const handle = handleEl.dataset.handle as HandleDir;
      interaction = { kind: 'resize', handle, startX: pt.x, startY: pt.y, orig: { ...box } };
      selQuickbar.classList.add('hidden');
      return;
    }

    const boxVisible = !selBox.classList.contains('hidden');
    const insideBox =
      boxVisible &&
      pt.x >= box.x &&
      pt.x <= box.x + box.w &&
      pt.y >= box.y &&
      pt.y <= box.y + box.h;

    if (insideBox && activeTool) {
      const lx = pt.x - box.x;
      const ly = pt.y - box.y;
      draftOp = { tool: activeTool, x1: lx, y1: ly, x2: lx, y2: ly };
      interaction = { kind: 'draw', tool: activeTool };
      selQuickbar.classList.add('hidden');
      return;
    }

    if (insideBox && !activeTool) {
      interaction = { kind: 'move', offsetX: pt.x - box.x, offsetY: pt.y - box.y };
      selQuickbar.classList.add('hidden');
      return;
    }

    ops = [];
    draftOp = null;
    activeTool = null;
    syncToolButtons();
    interaction = { kind: 'select', startX: pt.x, startY: pt.y };
    box = { x: pt.x, y: pt.y, w: 0, h: 0 };
    selBox.classList.remove('hidden');
    selQuickbar.classList.add('hidden');
    selBox.style.left = `${pt.x}px`;
    selBox.style.top = `${pt.y}px`;
    selBox.style.width = '0px';
    selBox.style.height = '0px';
  });

  stage.addEventListener('pointermove', (e) => {
    const target = e.target as HTMLElement;
    const overUi =
      target.closest('#ocr-hud') ||
      target.closest('#selection-quickbar') ||
      target.closest('#pinned-stage-card');
    const pt = stageCoords(e);

    if (overUi && interaction.kind === 'none') {
      if (guideH) guideH.style.opacity = '0';
      if (guideV) guideV.style.opacity = '0';
      if (loupe) loupe.style.opacity = '0';
      stage.style.cursor = 'default';
      return;
    }

    stage.style.cursor = '';
    if (guideH) {
      guideH.style.opacity = '1';
      guideH.style.top = `${pt.y}px`;
    }
    if (guideV) {
      guideV.style.opacity = '1';
      guideV.style.left = `${pt.x}px`;
    }
    if (loupe && loupeCoords && loupeHint) {
      loupe.style.opacity = '1';
      const lx = pt.x + 16 + 150 > pt.w ? pt.x - 156 : pt.x + 16;
      const ly = pt.y + 16 + 32 > pt.h ? pt.y - 34 : pt.y + 16;
      loupe.style.transform = `translate3d(${lx}px, ${ly}px, 0)`;
      loupeCoords.textContent = `${Math.round(pt.x)}, ${Math.round(pt.y)}`;
      const hoverTarget = target.closest<HTMLElement>('.capture-target');
      loupeHint.textContent = activeTool
        ? `Draw ${activeTool}`
        : (hoverTarget?.dataset.hint ?? 'Drag to snip');
    }

    if (interaction.kind === 'select') {
      box = {
        x: Math.min(interaction.startX, pt.x),
        y: Math.min(interaction.startY, pt.y),
        w: Math.abs(pt.x - interaction.startX),
        h: Math.abs(pt.y - interaction.startY),
      };
      selBox.style.left = `${box.x}px`;
      selBox.style.top = `${box.y}px`;
      selBox.style.width = `${box.w}px`;
      selBox.style.height = `${box.h}px`;
      selDims.textContent = `${Math.round(box.w)} × ${Math.round(box.h)} px`;
    } else if (interaction.kind === 'move') {
      box.x = pt.x - interaction.offsetX;
      box.y = pt.y - interaction.offsetY;
      applyBoxRect();
    } else if (interaction.kind === 'resize') {
      box = resizeBox(interaction.orig, interaction.handle, pt.x - interaction.startX, pt.y - interaction.startY);
      applyBoxRect();
    } else if (interaction.kind === 'draw' && draftOp) {
      draftOp.x2 = Math.max(0, Math.min(box.w, pt.x - box.x));
      draftOp.y2 = Math.max(0, Math.min(box.h, pt.y - box.y));
      syncCanvasAndDraw();
    }
  });

  window.addEventListener('pointerup', () => {
    if (interaction.kind === 'none') return;
    const finished = interaction;
    interaction = { kind: 'none' };

    if (finished.kind === 'draw') {
      if (draftOp && Math.hypot(draftOp.x2 - draftOp.x1, draftOp.y2 - draftOp.y1) > 6) {
        ops.push(draftOp);
        sound.play('tick');
      }
      draftOp = null;
      syncCanvasAndDraw();
      syncToolButtons();
      selQuickbar.classList.remove('hidden');
      return;
    }

    if (finished.kind === 'move' || finished.kind === 'resize') {
      selQuickbar.classList.remove('hidden');
      return;
    }

    if (finished.kind === 'select') {
      const boxRect = selBox.getBoundingClientRect();
      if (boxRect.width < 20 || boxRect.height < 20) {
        const targets = Array.from(document.querySelectorAll<HTMLElement>('.capture-target'));
        let best: TargetKey = 'code';
        let minDist = Infinity;
        targets.forEach((t) => {
          const r = t.getBoundingClientRect();
          const d = Math.hypot((r.left + r.right) / 2 - boxRect.left, (r.top + r.bottom) / 2 - boxRect.top);
          if (d < minDist && t.dataset.targetId) {
            minDist = d;
            best = t.dataset.targetId as TargetKey;
          }
        });
        autoSnipTarget(best);
        return;
      }

      let bestTarget: TargetKey = 'code';
      let maxArea = -1;
      document.querySelectorAll<HTMLElement>('.capture-target').forEach((t) => {
        const r = t.getBoundingClientRect();
        const ix = Math.max(0, Math.min(boxRect.right, r.right) - Math.max(boxRect.left, r.left));
        const iy = Math.max(0, Math.min(boxRect.bottom, r.bottom) - Math.max(boxRect.top, r.top));
        if (ix * iy > maxArea && t.dataset.targetId) {
          maxArea = ix * iy;
          bestTarget = t.dataset.targetId as TargetKey;
        }
      });

      selQuickbar.classList.remove('hidden');
      applyBoxRect();
      fireLaser();
      renderHud(bestTarget, true);
    }
  });

  hudCopyBtn?.addEventListener('click', () => {
    copyText(TARGETS[activeTarget].formats[activeFormat], `Copied ${activeFormat}`);
    if (hudCopyLabel) {
      hudCopyLabel.textContent = 'Copied ✓';
      window.setTimeout(() => (hudCopyLabel.textContent = 'Copy'), 1400);
    }
  });

  renderHud('code', false);
  syncToolButtons();
  window.setTimeout(() => autoSnipTarget('code'), 820);
}

function resizeBox(orig: BoxRect, handle: HandleDir, dx: number, dy: number): BoxRect {
  const minW = 64;
  const minH = 48;
  let { x, y, w, h } = orig;

  if (handle.includes('e')) w = Math.max(minW, orig.w + dx);
  if (handle.includes('s')) h = Math.max(minH, orig.h + dy);
  if (handle.includes('w')) {
    const clampedDx = Math.min(dx, orig.w - minW);
    x = orig.x + clampedDx;
    w = orig.w - clampedDx;
  }
  if (handle.includes('n')) {
    const clampedDy = Math.min(dy, orig.h - minH);
    y = orig.y + clampedDy;
    h = orig.h - clampedDy;
  }
  return { x, y, w, h };
}

function makeStarterOp(tool: Exclude<DrawTool, null>, box: BoxRect): DrawOp {
  if (tool === 'arrow') {
    return { tool, x1: box.w * 0.16, y1: box.h * 0.72, x2: box.w * 0.56, y2: box.h * 0.36 };
  }
  if (tool === 'mosaic') {
    return { tool, x1: box.w * 0.12, y1: box.h * 0.44, x2: box.w * 0.66, y2: box.h * 0.64 };
  }
  return { tool, x1: box.w * 0.12, y1: box.h * 0.28, x2: box.w * 0.72, y2: box.h * 0.62 };
}

function paintOp(ctx: CanvasRenderingContext2D, op: DrawOp): void {
  if (op.tool === 'rect') {
    const x = Math.min(op.x1, op.x2);
    const y = Math.min(op.y1, op.y2);
    const w = Math.abs(op.x2 - op.x1);
    const h = Math.abs(op.y2 - op.y1);
    ctx.strokeStyle = '#EF4444';
    ctx.lineWidth = 2.5;
    ctx.strokeRect(x, y, w, h);
    return;
  }

  if (op.tool === 'arrow') {
    const dx = op.x2 - op.x1;
    const dy = op.y2 - op.y1;
    const len = Math.hypot(dx, dy);
    if (len < 4) return;
    const ux = dx / len;
    const uy = dy / len;
    const headLen = Math.min(18, len * 0.55);
    const halfW = headLen * 0.55;

    ctx.strokeStyle = '#EF4444';
    ctx.fillStyle = '#EF4444';
    ctx.lineWidth = 3;
    ctx.lineCap = 'round';
    ctx.beginPath();
    ctx.moveTo(op.x1, op.y1);
    ctx.lineTo(op.x2 - ux * headLen * 0.7, op.y2 - uy * headLen * 0.7);
    ctx.stroke();

    const bx = op.x2 - ux * headLen;
    const by = op.y2 - uy * headLen;
    ctx.beginPath();
    ctx.moveTo(op.x2, op.y2);
    ctx.lineTo(bx - uy * halfW, by + ux * halfW);
    ctx.lineTo(bx + uy * halfW, by - ux * halfW);
    ctx.closePath();
    ctx.fill();
    return;
  }

  const x0 = Math.min(op.x1, op.x2);
  const y0 = Math.min(op.y1, op.y2);
  const w = Math.abs(op.x2 - op.x1);
  const h = Math.abs(op.y2 - op.y1);
  const step = 10;
  for (let py = y0; py < y0 + h; py += step) {
    for (let px = x0; px < x0 + w; px += step) {
      const hash = ((Math.floor(px / step) * 31 + Math.floor(py / step) * 17) & 3);
      const shades = ['rgba(51,65,85,0.92)', 'rgba(100,116,139,0.92)', 'rgba(30,41,59,0.94)', 'rgba(148,163,184,0.88)'];
      ctx.fillStyle = shades[hash];
      ctx.fillRect(px, py, Math.min(step, x0 + w - px), Math.min(step, y0 + h - py));
    }
  }
}
