import { sound } from '../motion/audio';
import { copyText } from './chrome';

type PipeKey = 'mosaic' | 'arrow' | 'note';
type LangKey = 'jp' | 'vi' | 'kr' | 'de';

const LANG_SAMPLES: Record<LangKey, { src: string; tr: string }> = {
  jp: {
    src: '光学認識エンジンは完全にオフラインで動作します。',
    tr: '→ "The optical recognition engine operates completely offline."',
  },
  vi: {
    src: 'Nhận diện tiếng Việt có dấu chuẩn xác hoàn toàn ngoại tuyến.',
    tr: '→ "Accurate Vietnamese diacritic recognition completely offline."',
  },
  kr: {
    src: '화면의 모든 텍스트를 로컬에서 즉시 추출하고 번역합니다.',
    tr: '→ "Instantly extracts and translates any on-screen text locally."',
  },
  de: {
    src: 'Präzise Texterkennung ohne Cloud-Übertragung direkt auf dem Gerät.',
    tr: '→ "Precision text recognition directly on-device with zero cloud transfer."',
  },
};

export function initBento(): void {
  initHeroTelemetry();
  initAnnotationPipeDemo();
  initPinOpacitySlider();
  initLangTabs();
  initColorSwatches();
}

function initHeroTelemetry(): void {
  const hero = document.getElementById('top');
  const coordEl = document.getElementById('hero-telemetry-coords');
  if (!hero || !coordEl) return;
  hero.addEventListener('pointermove', (e) => {
    const r = hero.getBoundingClientRect();
    const x = Math.max(0, Math.round(e.clientX - r.left));
    const y = Math.max(0, Math.round(e.clientY - r.top));
    coordEl.textContent = `X ${String(x).padStart(4, '0')} · Y ${String(y).padStart(4, '0')}`;
  });
}

function initAnnotationPipeDemo(): void {
  const preview = document.getElementById('pipe-preview');
  if (!preview) return;
  const state: Record<PipeKey, boolean> = { mosaic: true, arrow: true, note: false };

  const render = () => {
    const keyLine = state.mosaic
      ? 'API_KEY = ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  [Mosaic 12px]'
      : 'API_KEY = sk_live_94f2a8c10b7d3e99';
    const arrowLine = state.arrow
      ? '↗ [Red Arrow 4.5px] → pointing to retry_limit = 3'
      : '  retry_limit = 3';
    const noteLine = state.note ? '\n[Segoe UI Note] "Ship after rotating key"' : '';
    preview.textContent = `${keyLine}\n${arrowLine}${noteLine}`;
  };

  document.querySelectorAll<HTMLButtonElement>('.pipe-toggle').forEach((btn) => {
    btn.addEventListener('click', () => {
      const key = btn.dataset.pipe as PipeKey | undefined;
      if (!key || !(key in state)) return;
      state[key] = !state[key];
      sound.play('tick');
      const active = state[key];
      btn.classList.toggle('is-active', active);
      btn.setAttribute('aria-checked', String(active));
      const label = btn.querySelector('.pipe-label');
      if (label) {
        label.classList.toggle('text-white', active);
        label.classList.toggle('text-white/60', !active);
      }
      render();
      preview.classList.remove('pipe-flash');
      void preview.offsetWidth;
      preview.classList.add('pipe-flash');
    });
  });

  render();
}

function initPinOpacitySlider(): void {
  const slider = document.getElementById('pin-opacity-slider') as HTMLInputElement | null;
  const card = document.getElementById('pin-demo-card');
  const valEl = document.getElementById('pin-opacity-val');
  const fillEl = document.getElementById('pin-slider-fill');
  if (!slider || !card || !valEl) return;

  const update = () => {
    const val = Number(slider.value);
    const pct = ((val - 20) / (100 - 20)) * 100;
    valEl.textContent = `${val}%`;
    card.style.opacity = String(val / 100);
    if (fillEl) fillEl.style.width = `${Math.max(10, pct)}%`;
  };
  slider.addEventListener('input', update);
  update();
}

function initLangTabs(): void {
  const srcEl = document.getElementById('lang-src');
  const trEl = document.getElementById('lang-tr');
  if (!srcEl || !trEl) return;

  document.querySelectorAll<HTMLButtonElement>('.lang-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const code = btn.dataset.lang as LangKey | undefined;
      if (!code || !(code in LANG_SAMPLES)) return;
      sound.play('tick');
      srcEl.textContent = LANG_SAMPLES[code].src;
      trEl.textContent = LANG_SAMPLES[code].tr;
    });
  });
}

function initColorSwatches(): void {
  const readout = document.getElementById('swatch-readout');
  const copyBtn = document.getElementById('swatch-copy-btn');
  const strokePath = document.getElementById('swatch-stroke-path');
  const strokeHead = document.getElementById('swatch-stroke-head');
  let selectedHex = '#EF4444';
  let selectedWidth = 3.5;

  const replayStroke = () => {
    if (strokePath) {
      strokePath.setAttribute('stroke', selectedHex);
      strokePath.setAttribute('stroke-width', String(selectedWidth));
      strokePath.classList.remove('ink-stroke-anim');
      void strokePath.getBoundingClientRect();
      strokePath.classList.add('ink-stroke-anim');
    }
    if (strokeHead) {
      strokeHead.setAttribute('fill', selectedHex);
    }
  };

  document.querySelectorAll<HTMLButtonElement>('.swatch-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      selectedHex = btn.dataset.hex ?? '#EF4444';
      const name = btn.dataset.name ?? '';
      sound.play('tick');
      document.querySelectorAll('.swatch-btn').forEach((b) => b.classList.remove('is-selected'));
      btn.classList.add('is-selected');
      if (readout) readout.textContent = `${selectedHex} · ${name}`;
      replayStroke();
    });
  });

  document.querySelectorAll<HTMLButtonElement>('.stroke-size-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      selectedWidth = Number(btn.dataset.width ?? '3.5');
      sound.play('tick');
      document.querySelectorAll('.stroke-size-btn').forEach((b) => {
        b.classList.remove('bg-white', 'text-black', 'font-semibold');
        b.classList.add('text-white/55', 'hover:text-white');
      });
      btn.classList.remove('text-white/55', 'hover:text-white');
      btn.classList.add('bg-white', 'text-black', 'font-semibold');
      replayStroke();
    });
  });

  copyBtn?.addEventListener('click', () => {
    copyText(selectedHex, `Copied ${selectedHex}`);
  });
}
