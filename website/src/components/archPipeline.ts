import { sound } from '../motion/audio';
import { prefersReducedMotion } from '../motion/spring';

export type ArchRouteMode = 'local' | 'byok';

interface NodeMeta {
  id: string;
  file: string;
  title: string;
  detail: string;
  timing: string;
}

const NODE_META: Record<string, NodeMeta> = {
  hotkey: {
    id: 'hotkey',
    file: 'src/hotkey.rs',
    title: 'Global Hotkey & System Tray',
    detail: 'Win32 message loop wakes the hidden eframe app via App::logic with 0% idle CPU.',
    timing: '< 1 ms',
  },
  gdi: {
    id: 'gdi',
    file: 'src/capture/screen.rs',
    title: 'Win32 GDI BitBlt Snapshot',
    detail: 'Per-Monitor DPI v2 virtual desktop capture across negative multi-monitor origins.',
    timing: '~15 ms',
  },
  overlay: {
    id: 'overlay',
    file: 'src/snip/overlay_ui.rs',
    title: 'Pre-warmed egui Glow Overlay',
    detail: '1:1 GPU texture backdrop, 8 resize handles, 6 vector/mosaic tools & floating toolbar.',
    timing: '~3 ms',
  },
  raster: {
    id: 'raster',
    file: 'src/snip/raster.rs + pin/mod.rs',
    title: 'RGBA Export & Floating Pins',
    detail: 'Flattens arrows, mosaic & Segoe UI text directly into Clipboard RGBA, PNG, or always-on-top Pin.',
    timing: '~4 ms',
  },
  ocr: {
    id: 'ocr',
    file: 'src/ocr.rs',
    title: 'Offline Windows.Media.Ocr',
    detail: 'Otsu thresholding & dark-mode contrast tuning run 100% locally via WinRT COM.',
    timing: '~28 ms',
  },
  egress: {
    id: 'egress',
    file: 'src/providers/byok_ai.rs',
    title: 'Optional Direct TLS (BYOK / Free)',
    detail: 'Only fires if you press T or E. Direct PC -> Gemini / OpenAI via rustls. Zero Reticle server.',
    timing: 'On click only',
  },
};

export function initArchPipeline(): void {
  const stage = document.getElementById('arch-pipeline-stage');
  if (!stage) return;

  const modeBtns = stage.querySelectorAll<HTMLButtonElement>('[data-arch-mode]');
  const nodes = stage.querySelectorAll<HTMLElement>('[data-arch-node]');
  const inspectorTitle = document.getElementById('arch-inspector-title');
  const inspectorFile = document.getElementById('arch-inspector-file');
  const inspectorDetail = document.getElementById('arch-inspector-detail');
  const inspectorTiming = document.getElementById('arch-inspector-timing');
  const egressBadge = document.getElementById('arch-egress-badge');
  const egressNode = stage.querySelector<HTMLElement>('[data-arch-node="egress"]');
  const wireEgress = document.getElementById('wire-ocr-egress');

  let mode: ArchRouteMode = 'local';
  const timers: number[] = [];

  const clearTimers = () => {
    while (timers.length) {
      const t = timers.pop();
      if (t !== undefined) window.clearTimeout(t);
    }
  };

  const selectNodeInfo = (key: string) => {
    const info = NODE_META[key];
    if (!info) return;
    nodes.forEach((n) => n.classList.toggle('is-selected', n.dataset.archNode === key));
    if (inspectorTitle) inspectorTitle.textContent = info.title;
    if (inspectorFile) inspectorFile.textContent = info.file;
    if (inspectorDetail) inspectorDetail.textContent = info.detail;
    if (inspectorTiming) inspectorTiming.textContent = info.timing;
  };

  const setMode = (next: ArchRouteMode) => {
    mode = next;
    modeBtns.forEach((b) => {
      const act = b.dataset.archMode === next;
      b.className = act
        ? 'px-2.5 py-1 rounded-md text-[11px] font-semibold bg-white text-black transition-colors whitespace-nowrap'
        : 'px-2.5 py-1 rounded-md text-[11px] font-medium text-white/55 hover:text-white transition-colors whitespace-nowrap';
    });

    const isLocal = next === 'local';
    if (egressBadge) {
      egressBadge.textContent = isLocal ? 'SEALED · 0 B EGRESS' : 'DIRECT TLS · ZERO PROXY';
      egressBadge.className = isLocal
        ? 'font-mono text-[10px] px-2 py-0.5 rounded bg-emerald/15 text-emerald border border-emerald/30'
        : 'font-mono text-[10px] px-2 py-0.5 rounded bg-accent/15 text-accent border border-accent/30';
    }
    egressNode?.classList.toggle('is-sealed', isLocal);
    wireEgress?.classList.toggle('is-sealed', isLocal);
  };

  const animatePacket = (wireId: string, color: string, durationMs: number) => {
    if (prefersReducedMotion()) return;
    const svg = document.getElementById('arch-wires-svg');
    const path = document.getElementById(wireId) as SVGPathElement | null;
    if (!svg || !path) return;

    const totalLen = path.getTotalLength();
    const dot = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    dot.setAttribute('r', '3.6');
    dot.setAttribute('fill', color);
    dot.setAttribute('filter', 'url(#archGlow)');
    svg.appendChild(dot);

    path.classList.add('is-hot');
    const start = performance.now();

    const step = (now: number) => {
      const t = Math.min(1, (now - start) / durationMs);
      const eased = 1 - Math.pow(1 - t, 2.2);
      const pt = path.getPointAtLength(eased * totalLen);
      dot.setAttribute('cx', pt.x.toFixed(1));
      dot.setAttribute('cy', pt.y.toFixed(1));
      if (t < 1) {
        requestAnimationFrame(step);
      } else {
        dot.remove();
        path.classList.remove('is-hot');
      }
    };
    requestAnimationFrame(step);
  };

  const pulseNode = (key: string) => {
    const el = stage.querySelector<HTMLElement>(`[data-arch-node="${key}"]`);
    if (!el) return;
    el.classList.remove('is-pulsing');
    void el.offsetWidth;
    el.classList.add('is-pulsing');
    selectNodeInfo(key);
    timers.push(window.setTimeout(() => el.classList.remove('is-pulsing'), 520));
  };

  const runTrace = () => {
    clearTimers();
    pulseNode('hotkey');
    animatePacket('wire-hotkey-gdi', '#FFFFFF', 220);

    timers.push(
      window.setTimeout(() => {
        pulseNode('gdi');
        animatePacket('wire-gdi-overlay', '#38BDF8', 240);
      }, 210),
    );

    timers.push(
      window.setTimeout(() => {
        pulseNode('overlay');
        animatePacket('wire-overlay-raster', '#34D399', 260);
        animatePacket('wire-overlay-ocr', '#34D399', 260);
      }, 440),
    );

    timers.push(
      window.setTimeout(() => {
        pulseNode('raster');
        pulseNode('ocr');
        if (mode === 'byok') {
          animatePacket('wire-ocr-egress', '#FF4F30', 280);
          timers.push(window.setTimeout(() => pulseNode('egress'), 270));
        }
      }, 700),
    );
  };

  modeBtns.forEach((btn) => {
    btn.addEventListener('click', () => {
      const m = btn.dataset.archMode as ArchRouteMode | undefined;
      if (!m) return;
      sound.play('tick');
      setMode(m);
      runTrace();
    });
  });

  nodes.forEach((node) => {
    node.addEventListener('click', () => {
      const key = node.dataset.archNode;
      if (!key) return;
      sound.play('tick');
      pulseNode(key);
    });
  });

  window.addEventListener('reticle:simulate-snip', () => {
    runTrace();
  });

  setMode('local');
  selectNodeInfo('overlay');
}
