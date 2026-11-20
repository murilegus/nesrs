// Plays the samples the emulator posts to it. Runs on the audio thread.

const REPORT_EVERY_BLOCKS = 8;

class NesAudio extends AudioWorkletProcessor {
  constructor() {
    super();
    this.chunks = [];
    this.offset = 0;
    this.queued = 0;
    this.last = 0;
    this.blocks = 0;
    // Drop audio rather than fall ever further behind (about a quarter second).
    this.limit = Math.floor(sampleRate / 4);
    this.port.onmessage = (event) => {
      const chunk = event.data;
      this.chunks.push(chunk);
      this.queued += chunk.length;
      while (this.queued > this.limit && this.chunks.length > 1) {
        this.queued -= this.chunks.shift().length - this.offset;
        this.offset = 0;
      }
    };
  }

  process(_inputs, outputs) {
    const out = outputs[0][0];
    for (let i = 0; i < out.length; i++) {
      const chunk = this.chunks[0];
      if (chunk) {
        this.last = chunk[this.offset++];
        this.queued--;
        if (this.offset >= chunk.length) {
          this.chunks.shift();
          this.offset = 0;
        }
      } else {
        // Underrun: fade out instead of clicking.
        this.last *= 0.98;
      }
      out[i] = this.last;
    }
    for (let channel = 1; channel < outputs[0].length; channel++) {
      outputs[0][channel].set(out);
    }
    // Tell the main thread how much is buffered so it can steer the rate.
    if (++this.blocks % REPORT_EVERY_BLOCKS === 0) {
      this.port.postMessage(this.queued);
    }
    return true;
  }
}

registerProcessor("nes-audio", NesAudio);
