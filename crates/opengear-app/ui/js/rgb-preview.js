import { clamp, num, safeHex } from "./utils.js";
import { S, local } from "./state.js";

function hexToRgb(hex) {
  const h = safeHex(hex).slice(1);
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

function hsvToRgb(h, s, v) {
  h = ((h % 360) + 360) % 360;
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let r = 0, g = 0, b = 0;
  if (h < 60) [r, g, b] = [c, x, 0];
  else if (h < 120) [r, g, b] = [x, c, 0];
  else if (h < 180) [r, g, b] = [0, c, x];
  else if (h < 240) [r, g, b] = [0, x, c];
  else if (h < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}

function lerpRgb(a, b, t) {
  t = clamp(t, 0, 1);
  return [0, 1, 2].map((k) => Math.round(a[k] + (b[k] - a[k]) * t));
}

function sampleEffect(rgb, keyCode, nx, ny, t) {
  const p1 = hexToRgb(rgb.primary);
  const p2 = hexToRgb(rgb.secondary);
  const sp = num(rgb.speed, 1);
  const dir = rgb.reverse ? -1 : 1;
  let c = p1;
  switch (rgb.kind) {
    case "Off": c = [12, 15, 22]; break;
    case "PerKey": c = rgb.per_key && keyCode && rgb.per_key[keyCode] ? hexToRgb(rgb.per_key[keyCode]) : p1; break;
    case "RainbowWave": c = hsvToRgb(nx * 360 - dir * t * 120 * sp, 1, 1); break;
    case "SpiralRainbow": {
      const dx = nx - 0.5;
      const dy = (ny - 0.5) * 0.45;
      c = hsvToRgb((Math.atan2(dy, dx) * 180) / Math.PI + Math.hypot(dx, dy) * 240 - dir * t * 140 * sp, 1, 1);
      break;
    }
    case "SpectrumCycle": c = hsvToRgb(t * 45 * sp, 1, 1); break;
    case "Breathing": {
      const ph = Math.sin(t * 2 * sp) * 0.5 + 0.5;
      const mix = lerpRgb(p1, p2, ph);
      const f = 0.18 + 0.82 * ph;
      c = mix.map((x) => Math.round(x * f));
      break;
    }
    case "ColorWave": c = lerpRgb(p1, p2, Math.sin(nx * Math.PI * 2 - dir * t * 3 * sp) * 0.5 + 0.5); break;
    case "Visor": {
      const scan = Math.sin(t * 1.8 * sp) * 0.5 + 0.5;
      const g = Math.max(0, 1 - Math.abs(nx - scan) * 5);
      c = lerpRgb(p2.map((x) => x * 0.15), p1, g * g);
      break;
    }
    case "Rain": {
      const seed = Math.abs(Math.sin(Math.floor(nx * 23) * 37.7 + 11.3));
      const d = (((t * 0.85 * sp + seed * 7) % 1) - ny + 1) % 1;
      const tail = Math.max(0, 1 - d * 3.2);
      c = lerpRgb(p2.map((x) => x * 0.12), p1, tail * tail);
      break;
    }
    case "Gradient": c = lerpRgb(p1, p2, rgb.reverse ? 1 - nx : nx); break;
    case "Thermal": c = [0, 212, 255]; break;
    default: c = p1;
  }
  const br = num(rgb.brightness, 1);
  return `rgb(${Math.round(c[0] * br)},${Math.round(c[1] * br)},${Math.round(c[2] * br)})`;
}

let lastStepMs = 0;

function stepPreview(nowMs) {
  requestAnimationFrame(stepPreview);
  if (document.visibilityState !== "visible" || !S) return;
  if (local.tab !== "lighting" && local.tab !== "input") return;
  if (nowMs - lastStepMs < 85) return;
  lastStepMs = nowMs;

  const keys = document.querySelectorAll(".kb-key");
  const canvasLeds = document.querySelectorAll(".canvas-led");
  if (!keys.length && !canvasLeds.length) return;

  const prof = S.config.profile;
  const rgb = prof.rgb;
  const t = (nowMs - local.animStart) / 1000;
  const staticEff = ["Static", "PerKey", "Off", "Gradient"].includes(rgb.kind);

  if (keys.length) {
    const sig = `k|${rgb.kind}|${rgb.primary}|${rgb.secondary}|${rgb.brightness}|${rgb.reverse}|${JSON.stringify(rgb.per_key || {})}|${prof.enable_rgb}`;
    if (!(staticEff && sig === local.lastPreviewSig)) {
      local.lastPreviewSig = sig;
      keys.forEach((el) => {
        el.style.borderColor = prof.enable_rgb === false
          ? "rgba(255,255,255,0.14)"
          : sampleEffect(rgb, el.dataset.code || "", Number(el.dataset.nx), Number(el.dataset.ny), t);
      });
    }
  }
  if (canvasLeds.length) {
    canvasLeds.forEach((el) => {
      el.style.background = sampleEffect(rgb, el.dataset.code || "", Number(el.dataset.cx), Number(el.dataset.cy), t);
    });
  }
}

export function startRgbPreviewLoop() {
  requestAnimationFrame(stepPreview);
}
