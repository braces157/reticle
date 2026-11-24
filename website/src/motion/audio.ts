export type ShutterKind = 'tick' | 'scan' | 'shutter' | 'pin';

class SoundEngine {
  private enabled = true;
  private ctx: AudioContext | null = null;

  public isEnabled(): boolean {
    return this.enabled;
  }

  public toggle(): boolean {
    this.enabled = !this.enabled;
    if (this.enabled) this.play('tick');
    return this.enabled;
  }

  public play(kind: ShutterKind): void {
    if (!this.enabled) return;
    try {
      if (!this.ctx) {
        const AudioCtor =
          window.AudioContext ||
          (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
        if (!AudioCtor) return;
        this.ctx = new AudioCtor();
      }
      if (this.ctx.state === 'suspended') {
        void this.ctx.resume();
      }
      const now = this.ctx.currentTime;

      if (kind === 'shutter') {
        const bufferSize = Math.floor(this.ctx.sampleRate * 0.032);
        const noiseBuf = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
        const data = noiseBuf.getChannelData(0);
        for (let i = 0; i < bufferSize; i++) {
          data[i] = (Math.random() * 2 - 1) * Math.exp(-i / (bufferSize * 0.22));
        }
        const noise = this.ctx.createBufferSource();
        noise.buffer = noiseBuf;
        const filter = this.ctx.createBiquadFilter();
        filter.type = 'bandpass';
        filter.frequency.setValueAtTime(2400, now);
        filter.Q.setValueAtTime(1.8, now);
        const nGain = this.ctx.createGain();
        nGain.gain.setValueAtTime(0.08, now);
        nGain.gain.exponentialRampToValueAtTime(0.001, now + 0.03);
        noise.connect(filter);
        filter.connect(nGain);
        nGain.connect(this.ctx.destination);
        noise.start(now);

        const body = this.ctx.createOscillator();
        const bGain = this.ctx.createGain();
        body.type = 'sine';
        body.frequency.setValueAtTime(190, now);
        body.frequency.exponentialRampToValueAtTime(68, now + 0.045);
        bGain.gain.setValueAtTime(0.07, now);
        bGain.gain.exponentialRampToValueAtTime(0.001, now + 0.048);
        body.connect(bGain);
        bGain.connect(this.ctx.destination);
        body.start(now);
        body.stop(now + 0.05);
        return;
      }

      if (kind === 'pin') {
        [880, 1318.5].forEach((freq, idx) => {
          if (!this.ctx) return;
          const osc = this.ctx.createOscillator();
          const gain = this.ctx.createGain();
          osc.type = 'sine';
          osc.frequency.setValueAtTime(freq, now + idx * 0.035);
          gain.gain.setValueAtTime(0.03, now + idx * 0.035);
          gain.gain.exponentialRampToValueAtTime(0.001, now + idx * 0.035 + 0.09);
          osc.connect(gain);
          gain.connect(this.ctx.destination);
          osc.start(now + idx * 0.035);
          osc.stop(now + idx * 0.035 + 0.095);
        });
        return;
      }

      const osc = this.ctx.createOscillator();
      const gain = this.ctx.createGain();
      if (kind === 'scan') {
        osc.type = 'triangle';
        osc.frequency.setValueAtTime(1180, now);
        osc.frequency.exponentialRampToValueAtTime(340, now + 0.05);
        gain.gain.setValueAtTime(0.055, now);
        gain.gain.exponentialRampToValueAtTime(0.001, now + 0.06);
        osc.connect(gain);
        gain.connect(this.ctx.destination);
        osc.start(now);
        osc.stop(now + 0.065);
      } else {
        osc.type = 'sine';
        osc.frequency.setValueAtTime(740, now);
        osc.frequency.exponentialRampToValueAtTime(520, now + 0.028);
        gain.gain.setValueAtTime(0.032, now);
        gain.gain.exponentialRampToValueAtTime(0.001, now + 0.032);
        osc.connect(gain);
        gain.connect(this.ctx.destination);
        osc.start(now);
        osc.stop(now + 0.036);
      }
    } catch {
      // Audio autoplay or sandbox restrictions fail silently.
    }
  }
}

export const sound = new SoundEngine();
