export interface SpringConfig {
  stiffness: number;
  damping: number;
  mass?: number;
  precision?: number;
}

export const SPRING_SNAPPY: SpringConfig = { stiffness: 320, damping: 28, mass: 1, precision: 0.01 };
export const SPRING_GENTLE: SpringConfig = { stiffness: 170, damping: 24, mass: 1, precision: 0.01 };
export const SPRING_ELASTIC: SpringConfig = { stiffness: 260, damping: 20, mass: 1, precision: 0.02 };

/**
 * Frame-rate-independent damped harmonic spring (safe across 60Hz, 144Hz, and 240Hz monitors).
 * Uses fixed 4ms sub-steps so large frame deltas never cause numerical overshoot.
 */
export class Spring {
  public value: number;
  public velocity = 0;
  public target: number;
  private readonly stiffness: number;
  private readonly damping: number;
  private readonly mass: number;
  private readonly precision: number;

  constructor(initial: number, config: SpringConfig = SPRING_SNAPPY) {
    this.value = initial;
    this.target = initial;
    this.stiffness = config.stiffness;
    this.damping = config.damping;
    this.mass = config.mass ?? 1;
    this.precision = config.precision ?? 0.01;
  }

  public snap(v: number): void {
    this.value = v;
    this.target = v;
    this.velocity = 0;
  }

  public isSettled(): boolean {
    return (
      Math.abs(this.target - this.value) <= this.precision &&
      Math.abs(this.velocity) <= this.precision * 2
    );
  }

  /** Advances the spring by `dtSec` seconds (clamped to 50ms max). Returns true if still moving. */
  public step(dtSec: number): boolean {
    const clamped = Math.min(Math.max(dtSec, 0), 0.05);
    if (clamped === 0) return !this.isSettled();

    const maxStep = 0.004;
    let remaining = clamped;
    while (remaining > 0) {
      const h = Math.min(remaining, maxStep);
      const force = -this.stiffness * (this.value - this.target);
      const damper = -this.damping * this.velocity;
      const accel = (force + damper) / this.mass;
      this.velocity += accel * h;
      this.value += this.velocity * h;
      remaining -= h;
    }

    if (this.isSettled()) {
      this.value = this.target;
      this.velocity = 0;
      return false;
    }
    return true;
  }
}

export function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Gates continuous animations / canvases via IntersectionObserver + Page Visibility
 * so offscreen sections use 0% CPU.
 */
export function observeVisibility(
  el: Element,
  onChange: (visible: boolean) => void,
  rootMargin = '120px',
): () => void {
  let intersecting = false;
  const emit = () => onChange(intersecting && !document.hidden);

  const observer = new IntersectionObserver(
    (entries) => {
      intersecting = entries.some((entry) => entry.isIntersecting);
      emit();
    },
    { rootMargin },
  );
  observer.observe(el);

  const onVis = () => emit();
  document.addEventListener('visibilitychange', onVis);

  return () => {
    observer.disconnect();
    document.removeEventListener('visibilitychange', onVis);
  };
}
