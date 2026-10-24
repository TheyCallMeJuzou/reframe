// The parts of gifenc that compose.ts uses (the package has no types of its own).
declare module 'gifenc' {
  export type Palette = number[][];

  export function quantize(rgba: Uint8Array, maxColors: number): Palette;
  export function applyPalette(rgba: Uint8Array, palette: Palette): Uint8Array;

  export interface FrameOptions {
    palette?: Palette;
    /** Milliseconds. */
    delay?: number;
    /** 0: forever. */
    repeat?: number;
    transparent?: boolean;
    transparentIndex?: number;
    /** GIF disposal method; 1 keeps the frame for the next one to draw over. */
    dispose?: number;
  }

  export interface Encoder {
    writeFrame(index: Uint8Array, width: number, height: number, options?: FrameOptions): void;
    finish(): void;
    bytes(): Uint8Array;
  }

  export function GIFEncoder(): Encoder;
}
