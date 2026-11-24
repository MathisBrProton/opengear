// OpenGear Pro — Universal Plug & Play Native WebView2 Frontend Controller
(() => {
  const SWATCHES = [
    ["ROG Crimson", "#FF1848"],
    ["Cyber Cyan", "#00D4FF"],
    ["Corsair Yellow", "#FFC400"],
    ["Neon Violet", "#9B3CFF"],
    ["Emerald", "#00EB91"],
    ["Solar Orange", "#FF6A00"],
    ["Ice White", "#F0F6FF"],
    ["Deep Blue", "#195AFF"],
  ];

  // IPC Promise bridge to Rust backend
  let nextReqId = 1;
  const pending = new Map();
  window.__ogReply = (id, res) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    if (res && res.ok) p.resolve(res.data);
    else p.reject(new Error((res && res.error) || "Erreur IPC"));
  };

  function rpc(cmd, payload = {}) {
    return new Promise((resolve, reject) => {
      const id = nextReqId++;
      pending.set(id, { resolve, reject });
      window.ipc.postMessage(JSON.stringify({ id, cmd, payload }));
    });
  }

  function toast(msg, isErr = false) {
    setStatus(msg);
    const root = document.getElementById("toast-root");
    const el = document.createElement("div");
    el.className = "toast" + (isErr ? " err" : "");
    el.textContent = msg;
    root.appendChild(el);
    setTimeout(() => el.remove(), 3400);
  }

  function setStatus(txt) {
    const el = document.getElementById("status-text");
    if (el) el.textContent = txt;
  }

  function esc(s) {
    return String(s ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  // Local UI state
  let S = null;
  let activeTab = "dashboard";
  let selectedCorsairIdx = 0;
  let logiSelection = { kind: "mouse", index: 0 }; // kind: mouse | keyboard | headset
  let selectedMouseVariant = 0;
  let selectedHeadsetVariant = 0;
  let kbViewMode = "interactive"; // interactive | photo
  let selectedMouseProfileIdx = 0;
  let selectedMouseBtnIdx = 0;
  let selectedCoolingIdx = 0;
  let selectedAsusZone = 0;
  let selectedKeyId = "ESC";
  let paintColor = "#00D4FF";
  let sl3Color = "#FF8C1A";
  let macroSteps = [
    { key: "LCTRL", down: true, delay_ms: 10 },
    { key: "C", down: true, delay_ms: 10 },
    { key: "C", down: false, delay_ms: 10 },
    { key: "LCTRL", down: false, delay_ms: 10 },
  ];
  let draftProfile = null;
  let eqDraft = [0, 0, 0, 0, 0];
  let animStart = performance.now();

  // Studio render & optional local asset helpers (always crisp even with ZERO vendor software installed!)
  function getMouseImageUrl() {
    const vars = S?.local_assets?.mouse_variants || [];
    if (vars.length) {
      const idx = Math.min(selectedMouseVariant, vars.length - 1);
      return { url: vars[idx].asset_url, official: true, variants: vars };
    }
    const builtin = [
      { label: "Noir Carbone", asset_url: "/assets/mouse_superlight.jpg" },
      { label: "Blanc Mat", asset_url: "/assets/mouse_superlight_white.jpg" },
    ];
    const idx = Math.min(selectedMouseVariant, builtin.length - 1);
    return { url: builtin[idx].asset_url, official: false, variants: builtin };
  }

  function getHeadsetImageUrl() {
    const vars = S?.local_assets?.headset_variants || [];
    if (vars.length) {
      const idx = Math.min(selectedHeadsetVariant, vars.length - 1);
      return { url: vars[idx].asset_url, official: true, variants: vars };
    }
    return { url: "/assets/headset_prox.jpg", official: false, variants: [] };
  }

  function getK70ImageUrl() {
    const imgs = S?.local_assets?.k70_images || [];
    if (imgs.length) {
      const wanted = S?.config?.use_azerty_labels ? "fr" : "na";
      return (
        imgs.find((x) => x.id === wanted)?.asset_url ||
        imgs.find((x) => x.id === "thumb")?.asset_url ||
        imgs[0].asset_url
      );
    }
    return "/assets/keyboard_k70_tkl.jpg";
  }

  function getSelectedCorsair() {
    const list = S?.corsair_devices || [];
    if (!list.length) return null;
    if (selectedCorsairIdx >= list.length) selectedCorsairIdx = 0;
    return list[selectedCorsairIdx];
  }

  function getSelectedLogiMouse() {
    const list = S?.logi_mice || [];
    if (!list.length) return null;
    const idx = Math.min(logiSelection.index, list.length - 1);
    return list[idx];
  }

  function getSelectedLogiHeadset() {
    const list = S?.logi_headsets || [];
    if (!list.length) return null;
    const idx = Math.min(logiSelection.index, list.length - 1);
    return list[idx];
  }

  function getSelectedLogiKeyboard() {
    const list = S?.logi_keyboards || [];
    if (!list.length) return null;
    const idx = Math.min(logiSelection.index, list.length - 1);
    return list[idx];
  }

  // Automatic USB/HID Hot-Plug push from Rust
  window.__ogHotplug = (newState) => {
    S = newState;
    syncDraftsFromState();
    updateSidebar();
    render();
    toast("Plug & Play : périphériques USB mis à jour automatiquement !");
  };

  // Live telemetry push from Rust every 1s (updates KPI DOM without full re-render!)
  window.__ogTelemetry = (tel) => {
    if (!S) return;
    S.telemetry = tel;
    const sensors = tel.sensors || {};
    const mini = document.getElementById("footer-tel-mini");
    const gTemp = tel.gpu?.temp_c != null ? `${Math.round(tel.gpu.temp_c)}°C` : "—";
    const cTemp = sensors.cpu_temp_c != null ? ` (${Math.round(sensors.cpu_temp_c)}°C)` : "";
    const pumpStr = sensors.aio_pump_rpm != null ? `  •  AIO ${sensors.aio_pump_rpm} RPM` : "";
    if (mini) {
      mini.textContent = `CPU ${Math.round(tel.cpu_usage_pct)}%${cTemp}  •  GPU ${gTemp}${pumpStr}  •  RAM ${tel.ram_used_gb.toFixed(1)} Go`;
    }
    const cpuEl = document.getElementById("kpi-cpu-val");
    if (cpuEl) {
      cpuEl.textContent = sensors.cpu_temp_c != null
        ? `${Math.round(sensors.cpu_temp_c)}°C`
        : `${Math.round(tel.cpu_usage_pct)}%`;
      const cpuSub = document.getElementById("kpi-cpu-sub");
      if (cpuSub) {
        cpuSub.textContent = sensors.cpu_temp_c != null
          ? `Charge ${Math.round(tel.cpu_usage_pct)}% • ${sensors.aio_pump_rpm ? "AIO " + sensors.aio_pump_rpm + " RPM" : "Ryzen Tdie"}`
          : "AMD Ryzen 5 7600X";
      }
      const bar = document.getElementById("kpi-cpu-bar");
      if (bar) bar.style.width = `${Math.min(100, tel.cpu_usage_pct)}%`;
    }
    const ramEl = document.getElementById("kpi-ram-val");
    if (ramEl) {
      ramEl.textContent = `${tel.ram_used_gb.toFixed(1)} Go`;
      const bar = document.getElementById("kpi-ram-bar");
      if (bar) bar.style.width = `${Math.min(100, tel.ram_usage_pct)}%`;
    }
    const gpuEl = document.getElementById("kpi-gpu-val");
    if (gpuEl && tel.gpu?.available) {
      gpuEl.textContent = `${Math.round(tel.gpu.temp_c ?? 0)}°C`;
      const sub = document.getElementById("kpi-gpu-sub");
      if (sub) {
        const extra = tel.gpu.power_watts != null ? `${Math.round(tel.gpu.power_watts)}W` : (tel.gpu.core_clock_mhz ? `${tel.gpu.core_clock_mhz} MHz` : "");
        sub.textContent = `RX 7900 XT • ${tel.gpu.activity_pct ?? 0}% ${extra}`;
      }
      const bar = document.getElementById("kpi-gpu-bar");
      if (bar) bar.style.width = `${Math.min(100, ((tel.gpu.temp_c ?? 0) / 95) * 100)}%`;
    }
    const coolGpu = document.getElementById("cool-live-gpu");
    if (coolGpu) {
      const parts = [];
      if (sensors.cpu_temp_c != null) parts.push(`CPU ${Math.round(sensors.cpu_temp_c)}°C`);
      if (tel.gpu?.temp_c != null) parts.push(`GPU ${Math.round(tel.gpu.temp_c)}°C`);
      if (sensors.aio_pump_rpm != null) parts.push(`Pompe ${sensors.aio_pump_rpm} RPM`);
      if (parts.length) coolGpu.textContent = parts.join(" • ");
    }
  };

  function syncDraftsFromState() {
    if (!S) return;
    // Ensure valid Logitech selection
    if (logiSelection.kind === "mouse" && !(S.logi_mice || []).length) {
      if ((S.logi_keyboards || []).length) logiSelection = { kind: "keyboard", index: 0 };
      else if ((S.logi_headsets || []).length) logiSelection = { kind: "headset", index: 0 };
    }
    const m = getSelectedLogiMouse();
    if (m?.onboard_profiles?.length) {
      if (selectedMouseProfileIdx >= m.onboard_profiles.length) {
        selectedMouseProfileIdx = 0;
      }
      draftProfile = JSON.parse(JSON.stringify(m.onboard_profiles[selectedMouseProfileIdx]));
    } else {
      draftProfile = null;
    }
    const h = getSelectedLogiHeadset();
    if (h?.equalizer?.gains_db) {
      eqDraft = [...h.equalizer.gains_db];
    }
  }

  function totalActiveDevices() {
    if (!S) return 0;
    return (
      (S.asus ? 1 : 0) +
      (S.corsair_devices?.length || 0) +
      (S.logi_mice?.length || 0) +
      (S.logi_keyboards?.length || 0) +
      (S.logi_headsets?.length || 0)
    );
  }

  function updateSidebar() {
    if (!S) return;
    const setBadge = (id, text, cls) => {
      const el = document.getElementById(id);
      if (!el) return;
      el.textContent = text || "";
      el.className = "nav-badge" + (cls ? " " + cls : "");
    };
    setBadge("badge-asus", S.asus ? "ON" : "OFF", S.asus ? "rog" : "");
    setBadge("badge-cooling", S.cooling.wmi_available ? "WMI" : "AIO", S.cooling.wmi_available ? "ok" : "amber");

    const cList = S.corsair_devices || [];
    if (cList.length > 1) {
      setBadge("badge-corsair", `${cList.length}x`, "ok");
    } else if (cList.length === 1) {
      const hz = cList[0].pollrate_hz;
      setBadge("badge-corsair", hz ? (hz >= 1000 ? `${hz / 1000}K` : `${hz}`) : "ON", "ok");
    } else setBadge("badge-corsair", "OFF", "");

    const logiCount = (S.logi_mice?.length || 0) + (S.logi_keyboards?.length || 0) + (S.logi_headsets?.length || 0);
    if (S.logi_mice?.[0]?.battery?.percent != null) {
      setBadge("badge-logi", `${S.logi_mice[0].battery.percent}%`, "cyan");
    } else if (logiCount > 0) {
      setBadge("badge-logi", `${logiCount}x`, "cyan");
    } else setBadge("badge-logi", "", "");

    const socdOn = !!S.config?.profile?.socd?.enabled;
    const socdSusp = !!S.system_prefs?.socd_suspended;
    if (socdOn) {
      setBadge("badge-macros", socdSusp ? "CS2" : "SOCD", socdSusp ? "amber" : "ok");
    } else {
      setBadge("badge-macros", S.input_running ? "ACTIF" : "", S.input_running ? "amber" : "");
    }

    const sel = document.getElementById("profile-select");
    sel.innerHTML = S.config.profiles
      .map((n) => `<option value="${esc(n)}" ${n === S.config.active_profile ? "selected" : ""}>${esc(n)}</option>`)
      .join("");
    document.getElementById("chk-azerty").checked = !!S.config.use_azerty_labels;
  }

  let configWarningShown = false;
  async function refreshState(msg) {
    S = await rpc("get_state");
    syncDraftsFromState();
    updateSidebar();
    render();
    if (msg) toast(msg);
    if (S.config_warning && !configWarningShown) {
      configWarningShown = true;
      toast(S.config_warning, true);
    }
  }

  async function runAction(cmd, payload, okMsg, rerender = true) {
    try {
      const data = await rpc(cmd, payload);
      if (data && data.config) {
        S = data;
        syncDraftsFromState();
        updateSidebar();
        if (rerender) render();
      }
      if (okMsg) toast(okMsg);
      return data;
    } catch (e) {
      toast(e.message || String(e), true);
      throw e;
    }
  }

  // ==================== RGB COLOR SAMPLER ====================
  function hexToRgb(hex) {
    const h = (hex || "#00D4FF").replace("#", "");
    return [parseInt(h.slice(0, 2), 16) || 0, parseInt(h.slice(2, 4), 16) || 0, parseInt(h.slice(4, 6), 16) || 0];
  }
  function hsvToRgb(h, s, v) {
    h = ((h % 360) + 360) % 360;
    const c = v * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = v - c;
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
    t = Math.max(0, Math.min(1, t));
    return [Math.round(a[0] + (b[0] - a[0]) * t), Math.round(a[1] + (b[1] - a[1]) * t), Math.round(a[2] + (b[2] - a[2]) * t)];
  }
  function sampleEffect(rgbCfg, ledIdx, nx, ny, t) {
    const p1 = hexToRgb(rgbCfg.primary), p2 = hexToRgb(rgbCfg.secondary);
    const sp = rgbCfg.speed || 1, dir = rgbCfg.reverse ? -1 : 1;
    let c = p1;
    switch (rgbCfg.kind) {
      case "Off": c = [12, 15, 22]; break;
      case "Static": c = p1; break;
      case "PerKey": c = rgbCfg.per_key?.[String(ledIdx)] ? hexToRgb(rgbCfg.per_key[String(ledIdx)]) : p1; break;
      case "RainbowWave": c = hsvToRgb(nx * 360 - dir * t * 120 * sp, 1, 1); break;
      case "SpiralRainbow": {
        const dx = nx - 0.5, dy = (ny - 0.5) * 0.45;
        const ang = (Math.atan2(dy, dx) * 180) / Math.PI;
        const dist = Math.hypot(dx, dy);
        c = hsvToRgb(ang + dist * 240 - dir * t * 140 * sp, 1, 1);
        break;
      }
      case "SpectrumCycle": c = hsvToRgb(t * 45 * sp, 1, 1); break;
      case "Breathing": {
        const ph = Math.sin(t * 2 * sp) * 0.5 + 0.5;
        const mix = lerpRgb(p1, p2, ph);
        const f = 0.18 + 0.82 * ph;
        c = [Math.round(mix[0] * f), Math.round(mix[1] * f), Math.round(mix[2] * f)];
        break;
      }
      case "ColorWave": {
        const w = Math.sin(nx * Math.PI * 2 - dir * t * 3 * sp) * 0.5 + 0.5;
        c = lerpRgb(p1, p2, w);
        break;
      }
      case "Visor": {
        const scan = Math.sin(t * 1.8 * sp) * 0.5 + 0.5;
        const g = Math.max(0, 1 - Math.abs(nx - scan) * 5);
        c = lerpRgb([p2[0]*0.15, p2[1]*0.15, p2[2]*0.15], p1, g * g);
        break;
      }
      case "Rain": {
        const seed = Math.abs(Math.sin(Math.floor(nx * 23) * 37.7 + 11.3));
        const dy = (((t * 0.85 * sp + seed * 7) % 1) - ny + 1) % 1;
        const tail = Math.max(0, 1 - dy * 3.2);
        c = lerpRgb([p2[0]*0.12, p2[1]*0.12, p2[2]*0.12], p1, tail * tail);
        break;
      }
      case "Gradient": c = lerpRgb(p1, p2, rgbCfg.reverse ? 1 - nx : nx); break;
    }
    const br = rgbCfg.brightness ?? 1;
    return `rgb(${Math.round(c[0]*br)},${Math.round(c[1]*br)},${Math.round(c[2]*br)})`;
  }

  let lastPreviewSig = "";
  setInterval(() => {
    if (!S || document.hidden) return;
    const rgb = S.config.profile.rgb;
    const isStaticEff = ["Static", "PerKey", "Off", "Gradient"].includes(rgb.kind);

    if (activeTab === "corsair" || activeTab === "macros") {
      const sig = `${activeTab}|${rgb.kind}|${rgb.primary}|${rgb.secondary}|${rgb.brightness}|${rgb.reverse}|${Object.keys(rgb.per_key || {}).length}`;
      if (isStaticEff && sig === lastPreviewSig) return;
      lastPreviewSig = sig;
      const t = (performance.now() - animStart) / 1000;
      document.querySelectorAll(".kb-key").forEach((el) => {
        const col = sampleEffect(rgb, Number(el.dataset.led), Number(el.dataset.nx), Number(el.dataset.ny), t);
        el.style.borderColor = col;
      });
    } else if (activeTab === "asus" && S.asus) {
      const animAsus = S.config.profile.sync_asus_aura
        ? !isStaticEff
        : ["Rainbow", "SpectrumCycle", "Wave"].includes(S.asus.hw_mode);
      const sig = `asus|${S.asus.hw_mode}|${S.asus.hw_color}|${S.config.profile.sync_asus_aura}|${rgb.kind}|${rgb.primary}`;
      if (!animAsus && sig === lastPreviewSig) return;
      lastPreviewSig = sig;
      const t = (performance.now() - animStart) / 1000;
      document.querySelectorAll(".mb-hotspot").forEach((el, idx) => {
        let col = S.asus.hw_color;
        if (S.config.profile.sync_asus_aura) {
          col = sampleEffect(rgb, idx, idx / 4, 0.5, t);
        } else if (["Rainbow", "SpectrumCycle", "Wave"].includes(S.asus.hw_mode)) {
          const [r, g, b] = hsvToRgb(t * 85 + idx * 55, 1, 1);
          col = `rgb(${r},${g},${b})`;
        } else if (S.asus.hw_mode === "Off") {
          col = "#364158";
        }
        el.style.setProperty("--zone-col", col);
      });
    }
  }, 100);

  // ==================== MAIN RENDER ROUTER ====================
  function render() {
    if (!S) return;
    lastPreviewSig = "";
    const main = document.getElementById("main-content");
    switch (activeTab) {
      case "dashboard": main.innerHTML = viewDashboard(); bindDashboard(); break;
      case "asus": main.innerHTML = viewAsus(); bindAsus(); break;
      case "cooling": main.innerHTML = viewCooling(); bindCooling(); break;
      case "corsair": main.innerHTML = viewCorsair(); bindCorsair(); break;
      case "logitech": main.innerHTML = viewLogitech(); bindLogitech(); break;
      case "macros": main.innerHTML = viewMacros(); bindMacros(); break;
      case "devices": main.innerHTML = viewDevices(); bindDevices(); break;
    }
  }

  // ==================== 1. DASHBOARD (DYNAMIC PLUG & PLAY SHOWCASE) ====================
  function viewDashboard() {
    const t = S.telemetry;
    const sensors = t.sensors || {};
    const prof = S.config.profile;
    const syncAll = prof.enable_rgb && prof.sync_asus_aura;
    const gpuTemp = t.gpu?.temp_c != null ? Math.round(t.gpu.temp_c) : null;
    const firstMouse = S.logi_mice?.[0];
    const mouseBat = firstMouse?.battery?.percent;
    const mouseHrs = firstMouse?.power_model?.estimated_hours;
    const mImg = getMouseImageUrl();
    const hImg = getHeadsetImageUrl();
    const k70Photo = getK70ImageUrl();

    // Build dynamic showcase cards for EVERY connected device across all brands!
    const cards = [];
    cards.push(`
      <button class="device-card" data-jump="asus">
        <span class="pill rog"><span class="dot"></span>${S.asus ? "Aura OK" : "Absent"}</span>
        <div class="thumb"><img src="/assets/motherboard_b650a.jpg" alt="ASUS ROG" /></div>
        <div><div class="dev-name">${esc(S.static_info.motherboard.replace(" GAMING WIFI", ""))}</div><div class="dev-meta">${S.asus ? `${S.asus.argb_5v_headers} ARGB Gen2 • ${S.asus.rgb_12v_headers} RGB 12V` : "Carte mère"}</div></div>
      </button>
    `);
    cards.push(`
      <button class="device-card" data-jump="cooling">
        <span class="pill amber"><span class="dot"></span>${sensors.aio_pump_rpm != null ? `${sensors.aio_pump_rpm} RPM` : (S.cooling.wmi_available ? "WMI Actif" : "PWM 3500 RPM")}</span>
        <div class="thumb"><img src="/assets/aio_silentloop3.jpg" alt="Silent Loop 3" /></div>
        <div><div class="dev-name">Silent Loop 3 AIO</div><div class="dev-meta">Pompe PWM + Courbes 8 pts</div></div>
      </button>
    `);

    (S.corsair_devices || []).forEach((dev, idx) => {
      const badge = dev.pollrate_hz ? `${dev.pollrate_hz} Hz` : (dev.battery_pct != null ? `${dev.battery_pct}%` : dev.kind_label);
      const isOfficialK70 = (S?.local_assets?.k70_images || []).length > 0;
      const thumb = dev.kind === "Mouse"
        ? `<img src="${mImg.url}" alt="${esc(dev.product_name)}" />`
        : dev.kind === "Headset"
          ? `<img src="${hImg.url}" alt="${esc(dev.product_name)}" />`
          : dev.kind === "CoolerHub"
            ? `<img src="/assets/aio_silentloop3.jpg" alt="${esc(dev.product_name)}" />`
            : `<img src="${k70Photo}" class="${isOfficialK70 ? "official-cutout" : ""}" alt="${esc(dev.product_name)}" />`;
      cards.push(`
        <button class="device-card" data-jump="corsair" data-cidx="${idx}">
          <span class="pill emerald"><span class="dot"></span>${esc(badge)}</span>
          <div class="thumb">${thumb}</div>
          <div><div class="dev-name">${esc(dev.product_name)}</div><div class="dev-meta">FW ${esc(dev.firmware)} • ${esc(dev.kind_label)}</div></div>
        </button>
      `);
    });

    (S.logi_mice || []).forEach((m, idx) => {
      cards.push(`
        <button class="device-card" data-jump="logitech" data-lkind="mouse" data-lidx="${idx}">
          <span class="pill cyan"><span class="dot"></span>${m.dpi?.dpi_x || 1600} DPI</span>
          <div class="thumb"><img src="${mImg.url}" class="${mImg.official ? "official-cutout" : ""}" alt="${esc(m.name)}" /></div>
          <div><div class="dev-name">${esc(m.name)}</div><div class="dev-meta">${m.battery?.percent != null ? `${m.battery.percent}% • ` : ""}${m.onboard?.profile_count || 0} profils Flash</div></div>
        </button>
      `);
    });

    (S.logi_keyboards || []).forEach((k, idx) => {
      cards.push(`
        <button class="device-card" data-jump="logitech" data-lkind="keyboard" data-lidx="${idx}">
          <span class="pill cyan"><span class="dot"></span>${k.battery?.percent != null ? `${k.battery.percent}%` : "HID++ RGB"}</span>
          <div class="thumb"><img src="/assets/keyboard_k70_tkl.jpg" alt="${esc(k.name)}" /></div>
          <div><div class="dev-name">${esc(k.name)}</div><div class="dev-meta">Clavier Logitech • HID++ v${esc(k.proto_version)}</div></div>
        </button>
      `);
    });

    (S.logi_headsets || []).forEach((h, idx) => {
      cards.push(`
        <button class="device-card" data-jump="logitech" data-lkind="headset" data-lidx="${idx}">
          <span class="pill purple"><span class="dot"></span>${h.equalizer ? `EQ ${h.equalizer.band_count} Bandes` : "Audio"}</span>
          <div class="thumb"><img src="${hImg.url}" class="${hImg.official ? "official-cutout" : ""}" alt="${esc(h.name)}" /></div>
          <div><div class="dev-name">${esc(h.name)}</div><div class="dev-meta">Sidetone ${h.sidetone_pct}% • 10 Presets Pro</div></div>
        </button>
      `);
    });

    const assetBadges = [];
    if (S.local_assets?.lghub_depot_found) assetBadges.push("Images G HUB locales");
    if (S.local_assets?.icue_devices_found) assetBadges.push("Images iCUE 5 (.rcc) locales");

    return `
      <div class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar"></div>
          <div>
            <div class="page-title">Tableau de bord système</div>
            <div class="page-sub">${esc(S.static_info.motherboard)}  •  BIOS ${esc(S.static_info.bios_version)}  •  ${esc(S.static_info.cpu_name)}</div>
          </div>
        </div>
        <div class="header-actions">
          ${assetBadges.length ? `<span class="pill cyan">${esc(assetBadges.join(" + "))}</span>` : ""}
          <span class="pill emerald"><span class="dot"></span>${totalActiveDevices()} périphériques Plug &amp; Play</span>
        </div>
      </div>

      <!-- LIVE TELEMETRY KPIs -->
      <div class="grid-4">
        <div class="card">
          <div class="card-eyebrow">${sensors.cpu_temp_c != null ? "Processeur (Tdie Live)" : "Processeur"}</div>
          <div class="kpi-val" id="kpi-cpu-val">${sensors.cpu_temp_c != null ? Math.round(sensors.cpu_temp_c) + "°C" : Math.round(t.cpu_usage_pct) + "%"}</div>
          <div class="muted" id="kpi-cpu-sub">${sensors.cpu_temp_c != null ? `Charge ${Math.round(t.cpu_usage_pct)}% • ${sensors.aio_pump_rpm ? "AIO " + sensors.aio_pump_rpm + " RPM" : "Ryzen 5 7600X"}` : "AMD Ryzen 5 7600X"}</div>
          <div class="bar-track"><div class="bar-fill" id="kpi-cpu-bar" style="width:${Math.min(100, t.cpu_usage_pct)}%;background:var(--cyan)"></div></div>
        </div>
        <div class="card">
          <div class="card-eyebrow">Carte Graphique (ADL2)</div>
          <div class="kpi-val" id="kpi-gpu-val" style="color:var(--rog)">${gpuTemp != null ? gpuTemp + "°C" : "RX 7900 XT"}</div>
          <div class="muted" id="kpi-gpu-sub">${t.gpu?.available ? `RX 7900 XT • ${t.gpu.activity_pct ?? 0}% ${t.gpu.core_clock_mhz ? t.gpu.core_clock_mhz + " MHz" : ""}` : "ADL2 indisponible"}</div>
          <div class="bar-track"><div class="bar-fill" id="kpi-gpu-bar" style="width:${gpuTemp != null ? Math.min(100, (gpuTemp/95)*100) : 0}%;background:var(--rog)"></div></div>
        </div>
        <div class="card">
          <div class="card-eyebrow">Mémoire DDR5</div>
          <div class="kpi-val" id="kpi-ram-val">${t.ram_used_gb.toFixed(1)} Go</div>
          <div class="muted">sur ${Math.round(t.ram_total_gb)} Go (${Math.round(t.ram_usage_pct)}%)</div>
          <div class="bar-track"><div class="bar-fill" id="kpi-ram-bar" style="width:${Math.min(100, t.ram_usage_pct)}%;background:var(--purple)"></div></div>
        </div>
        <div class="card">
          <div class="card-eyebrow">Autonomie Souris</div>
          <div class="kpi-val" style="color:var(--emerald)">${mouseBat != null ? mouseBat + "%" : "—"}</div>
          <div class="muted">${mouseHrs != null ? `~${mouseHrs.toFixed(1)} h restantes • ${firstMouse.report_rate?.current_hz || 1000} Hz` : "PRO X Wireless"}</div>
          <div class="bar-track"><div class="bar-fill" style="width:${mouseBat ?? 0}%;background:var(--emerald)"></div></div>
        </div>
      </div>

      <!-- DYNAMIC PLUG & PLAY DEVICE SHOWCASE -->
      <div class="mt-16">
        <div class="card-eyebrow" style="margin-bottom:8px">TON SETUP DÉTECTÉ AUTOMATIQUEMENT (HOT-PLUG ACTIF) — CLIQUE POUR CONFIGURER</div>
        <div class="showcase-grid" style="grid-template-columns:repeat(${Math.min(Math.max(cards.length, 3), 5)}, minmax(0, 1fr))">
          ${cards.join("")}
        </div>
      </div>

      <!-- OPENGEAR UNIFIED RGB SYNC -->
      <div class="card accent-cyan mt-16">
        <div class="row-between">
          <div>
            <div class="card-title">OpenGear Sync — Synchronisation RGB Multi-Marques (incl. Ambilight &amp; Frappe Réactive)</div>
            <div class="muted">Unifie la carte mère ASUS ROG (headers ARGB inclus) et tous tes claviers Corsair Bragi en temps réel.</div>
          </div>
          <label class="switch">
            <input type="checkbox" id="dash-sync-all" ${syncAll ? "checked" : ""} />
            <span class="switch-track"></span>
            <span>Sync Tout</span>
          </label>
        </div>
        <div class="row mt-12">
          <select id="dash-eff" class="og-select" style="width:230px">
            ${S.effects.map((e) => `<option value="${e.id}" ${e.id === prof.rgb.kind ? "selected" : ""}>${esc(e.label)}</option>`).join("")}
          </select>
          <input type="color" id="dash-c1" class="og-color" value="${prof.rgb.primary}" title="Couleur primaire" />
          <input type="color" id="dash-c2" class="og-color" value="${prof.rgb.secondary}" title="Couleur secondaire" />
          <div style="width:150px;display:flex;align-items:center;gap:8px">
            <span class="muted">Vitesse</span>
            <input type="range" id="dash-spd" class="og-range" min="0.2" max="4" step="0.1" value="${prof.rgb.speed}" />
          </div>
          <div class="swatch-row" style="margin-left:auto">
            ${SWATCHES.map(([n, hex]) => `<button class="swatch ${prof.rgb.primary.toUpperCase() === hex ? "active" : ""}" data-dash-swatch="${hex}" style="background:${hex}" title="${n}"></button>`).join("")}
          </div>
        </div>
      </div>

      <!-- SYSTEM TRAY, AUTOSTART, SHORTCUTS & AUTO-PROFILE SWITCH -->
      ${(() => {
        const sp = S.system_prefs || {};
        return `
          <div class="card accent-purple mt-16">
            <div class="card-head">
              <div>
                <div class="card-title">System Tray, Lancement Rapide &amp; Auto-Switch Jeux</div>
                <div class="muted">Icône OpenGear dans la zone de notification Windows • Arrière-plan ultra-léger • Bascule auto selon le jeu actif</div>
              </div>
              <div class="row">
                ${sp.foreground_app ? `<span class="pill cyan">App active : ${esc(sp.foreground_app)}</span>` : ""}
                ${sp.auto_switched ? `<span class="pill emerald">Auto-Switch actif</span>` : ""}
                <button class="btn btn-sm btn-primary" id="dash-create-shortcuts">Créer raccourcis Bureau &amp; Démarrer</button>
              </div>
            </div>
            <div class="row" style="gap:20px;flex-wrap:wrap">
              <label class="switch">
                <input type="checkbox" id="pref-min-tray" ${sp.minimize_to_tray !== false ? "checked" : ""} />
                <span class="switch-track"></span>
                <span>Réduire dans le System Tray en fermant (X)</span>
              </label>
              <label class="switch">
                <input type="checkbox" id="pref-autostart" ${sp.autostart ? "checked" : ""} />
                <span class="switch-track"></span>
                <span>Démarrer avec Windows</span>
              </label>
              <label class="switch">
                <input type="checkbox" id="pref-start-min" ${sp.start_minimized ? "checked" : ""} />
                <span class="switch-track"></span>
                <span>Démarrer caché dans le Tray</span>
              </label>
              <label class="switch">
                <input type="checkbox" id="pref-auto-prof" ${sp.auto_profile_switch !== false ? "checked" : ""} />
                <span class="switch-track"></span>
                <span>Auto-Switch de profil par jeu</span>
              </label>
            </div>
          </div>
        `;
      })()}

      <!-- WINDOWS POWER PLANS -->
      <div class="card mt-16">
        <div class="card-head">
          <div class="card-title">Profils de Performance Système (Windows Power Plans)</div>
        </div>
        <div class="plan-grid">
          ${S.power_plans.map((p) => {
            const isPerf = /Bitsum|Haute/i.test(p.name);
            return `
              <div class="plan-card ${p.active ? "active" : ""} ${isPerf ? "perf" : ""}">
                <div>
                  <div style="font-weight:650;color:#fff">${esc(p.name)}</div>
                  <div class="mono muted" style="font-size:10px;margin-top:3px">${esc(p.guid)}</div>
                </div>
                <div>
                  ${p.active
                    ? `<span class="pill ${isPerf ? "rog" : "cyan"}"><span class="dot"></span>ACTIF</span>`
                    : `<button class="btn btn-sm btn-secondary" data-power-guid="${esc(p.guid)}">Activer</button>`}
                </div>
              </div>
            `;
          }).join("")}
        </div>
      </div>
    `;
  }

  function bindDashboard() {
    document.querySelectorAll(".device-card").forEach((b) => {
      b.onclick = () => {
        activeTab = b.dataset.jump;
        if (b.dataset.cidx != null) selectedCorsairIdx = Number(b.dataset.cidx);
        if (b.dataset.lkind) {
          logiSelection = { kind: b.dataset.lkind, index: Number(b.dataset.lidx || 0) };
          syncDraftsFromState();
        }
        highlightNav();
        render();
      };
    });
    document.getElementById("dash-sync-all").onchange = (e) =>
      runAction("set_sync_all", { enabled: e.target.checked }, e.target.checked ? "OpenGear Sync activée sur ASUS ROG + Corsair !" : "Sync désactivée — retour modes matériels.");
    const pushRgb = () =>
      runAction("set_rgb_config", {
        kind: document.getElementById("dash-eff").value,
        primary: document.getElementById("dash-c1").value,
        secondary: document.getElementById("dash-c2").value,
        speed: parseFloat(document.getElementById("dash-spd").value),
      }, null, false);
    ["dash-eff", "dash-c1", "dash-c2", "dash-spd"].forEach((id) => {
      document.getElementById(id).oninput = pushRgb;
    });
    document.querySelectorAll("[data-dash-swatch]").forEach((s) => {
      s.onclick = () => {
        document.getElementById("dash-c1").value = s.dataset.dashSwatch;
        runAction("set_rgb_config", { primary: s.dataset.dashSwatch }, `Couleur ${s.dataset.dashSwatch} appliquée`);
      };
    });
    document.querySelectorAll("[data-power-guid]").forEach((b) => {
      b.onclick = () => runAction("set_power_plan", { guid: b.dataset.powerGuid }, "Mode d'alimentation Windows activé");
    });
    const prefMinTray = document.getElementById("pref-min-tray");
    if (prefMinTray) prefMinTray.onchange = (e) => runAction("sys_set_prefs", { minimize_to_tray: e.target.checked }, e.target.checked ? "La croix (X) réduira OpenGear dans le System Tray" : "La croix (X) quittera OpenGear");
    const prefAutostart = document.getElementById("pref-autostart");
    if (prefAutostart) prefAutostart.onchange = (e) => runAction("sys_set_prefs", { autostart: e.target.checked }, e.target.checked ? "OpenGear démarrera automatiquement avec Windows !" : "Démarrage automatique Windows désactivé");
    const prefStartMin = document.getElementById("pref-start-min");
    if (prefStartMin) prefStartMin.onchange = (e) => runAction("sys_set_prefs", { start_minimized: e.target.checked }, e.target.checked ? "Démarrage discret dans le System Tray activé" : "OpenGear s'ouvrira visible au lancement");
    const prefAutoProf = document.getElementById("pref-auto-prof");
    if (prefAutoProf) prefAutoProf.onchange = (e) => runAction("sys_set_prefs", { auto_profile_switch: e.target.checked }, e.target.checked ? "Auto-Switch de profil par jeu activé !" : "Auto-Switch de profil désactivé");
    const btnSc = document.getElementById("dash-create-shortcuts");
    if (btnSc) btnSc.onclick = () => runAction("sys_create_shortcuts", {}, "Raccourcis Bureau & Menu Démarrer créés avec l'icône OpenGear !", false);
  }

  // ==================== 2. ASUS ROG AURA ====================
  function viewAsus() {
    if (!S.asus) {
      return `
        <div class="page-header"><div class="page-title-wrap"><div class="page-bar rog"></div><div><div class="page-title">ASUS ROG — Carte Mère &amp; Aura Sync</div><div class="page-sub">${esc(S.asus_err || "Contrôleur absent")}</div></div></div></div>
        <div class="card"><button class="btn btn-rog" id="asus-retry">Réessayer la connexion</button></div>`;
    }
    const a = S.asus;
    const direct = S.config.profile.sync_asus_aura;
    const hotspots = [
      { ch: 0, label: "ROG IO / VRM", x: 18, y: 28 },
      { ch: 0, label: "RAM DDR5 RGB", x: 58, y: 34 },
      { ch: 0, label: "Chipset B650-A", x: 78, y: 69 },
      { ch: 1, label: "ARGB #1", x: 75, y: 12 },
      { ch: 2, label: "ARGB #2", x: 79, y: 55 },
      { ch: 3, label: "ARGB #3", x: 32, y: 92 },
    ];
    const ram = S.ram_rgb || { is_admin: false, wmi_smbus_available: false, physical_dimms: [], ene_modules: [], message: "", last_mode: "Static", last_color: "#00D2FF", last_speed: 2 };
    const ramModes = S.ram_rgb_modes || [];
    const totalRamGb = (ram.physical_dimms || []).reduce((acc, d) => acc + (d.capacity_gb || 0), 0);
    const ramSpeed = ram.physical_dimms?.[0]?.speed_mts || 0;

    return `
      <div class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar rog"></div>
          <div>
            <div class="page-title">ASUS ROG — Carte Mère &amp; Aura Sync</div>
            <div class="page-sub">${esc(S.static_info.motherboard)}  •  BIOS ${esc(S.static_info.bios_version)}  •  Remplacement léger d'Armoury Crate</div>
          </div>
        </div>
        <div class="header-actions">
          <span class="pill rog"><span class="dot"></span>${esc(a.firmware)}</span>
          <label class="switch rog">
            <input type="checkbox" id="asus-direct-toggle" ${direct ? "checked" : ""} />
            <span class="switch-track"></span>
            <span>Mode Direct (OpenGear Sync)</span>
          </label>
        </div>
      </div>

      <div class="grid-stage">
        <div class="stack">
          <div class="hero-stage rog-bg" style="padding:18px">
            <div class="stage-img-wrap">
              <img class="stage-img" src="/assets/motherboard_b650a.jpg" alt="ASUS ROG STRIX B650-A" style="max-height:460px" />
              ${hotspots.map((h, i) => `
                <button class="mb-hotspot ${selectedAsusZone === i ? "active" : ""}" data-zone="${i}" style="left:${h.x}%;top:${h.y}%;--zone-col:${a.hw_color}">
                  <span class="led-dot"></span>${h.label}
                </button>
              `).join("")}
            </div>
          </div>
          <div class="card">
            <table class="meta-table">
              <tr><td class="muted">Carte Mère</td><td>${esc(S.static_info.motherboard)}</td></tr>
              <tr><td class="muted">Contrôleur HID</td><td class="mono">${a.vid}:${a.pid} (${esc(a.firmware)})</td></tr>
              <tr><td class="muted">Topologie Aura</td><td>${a.mainboard_fixed_leds} LEDs CM • ${a.rgb_12v_headers} RGB 12V • ${a.argb_5v_headers} ARGB 5V Gen2</td></tr>
            </table>
          </div>
        </div>

        <div class="stack">
          <div class="card accent-rog">
            <div class="card-head">
              <div class="card-title">Effets Matériels MCU Aura (autonomes sans logiciel)</div>
              <span class="pill rog">${esc(a.hw_mode)}</span>
            </div>
            <div class="seg-group" style="width:100%">
              ${S.aura_modes.map((m) => `<button class="seg-btn rog ${a.hw_mode === m.id ? "active" : ""}" data-aura-mode="${m.id}">${esc(m.label)}</button>`).join("")}
            </div>
            <div class="row mt-12">
              <span class="muted">Couleur :</span>
              <input type="color" id="asus-color" class="og-color" value="${a.hw_color}" />
              <div class="swatch-row">
                ${SWATCHES.map(([n, hex]) => `<button class="swatch ${a.hw_color.toUpperCase() === hex ? "active" : ""}" data-asus-swatch="${hex}" style="background:${hex}" title="${n}"></button>`).join("")}
              </div>
            </div>
            <div class="row mt-12">
              <button class="btn btn-rog" id="asus-apply-hw">Appliquer en direct</button>
              <button class="btn btn-secondary" id="asus-commit-hw">Enregistrer en Flash (persistant au boot)</button>
            </div>
          </div>

          <div class="card">
            <div class="card-head">
              <div class="card-title">Mode PC Éteint (Shutdown / Stealth Mode)</div>
              <span class="muted">Stocké dans l'EEPROM de la carte mère</span>
            </div>
            <div class="row">
              <select id="asus-sd-mode" class="og-select" style="width:180px">
                <option value="Off" ${a.shutdown_mode === "Off" ? "selected" : ""}>Éteint (Stealth)</option>
                <option value="Static" ${a.shutdown_mode === "Static" ? "selected" : ""}>Statique</option>
                <option value="Breathing" ${a.shutdown_mode === "Breathing" ? "selected" : ""}>Respiration</option>
                <option value="Rainbow" ${a.shutdown_mode === "Rainbow" ? "selected" : ""}>Arc-en-ciel</option>
              </select>
              <input type="color" id="asus-sd-color" class="og-color" value="${a.shutdown_color}" />
              <button class="btn btn-secondary" id="asus-apply-sd">Appliquer + Commit EEPROM</button>
            </div>
          </div>

          <div class="card">
            <div class="card-head">
              <div class="card-title">Calibration des Headers 5V ARGB Gen2</div>
              <span class="muted">Nombre de LEDs par header pour le streaming Direct</span>
            </div>
            <div class="stack" style="gap:10px">
              ${a.channels.filter((c) => c.kind === "argb").map((c) => `
                <div class="row-between">
                  <div style="min-width:155px;font-weight:600">${esc(c.name)}</div>
                  <input type="range" class="og-range" style="flex:1" min="4" max="90" value="${c.led_count}" data-argb-ch="${c.index}" />
                  <span class="mono" style="width:58px;text-align:right" id="argb-lbl-${c.index}">${c.led_count} LEDs</span>
                </div>
              `).join("")}
            </div>
          </div>

          <div class="card accent-cyan">
            <div class="card-head">
              <div>
                <div class="card-title">Mémoire RAM DDR4 / DDR5 RGB (Universel PawnIO AMD &amp; Intel + ASUS WMI + OpenRGB SDK)</div>
                <div class="muted">${ram.physical_dimms.length} barrette(s) • ${totalRamGb} Go ${ram.physical_dimms[0]?.memory_type || "DDR5"}${ramSpeed ? ` @ ${ramSpeed} MT/s` : ""} — Compatible toutes cartes mères via pilote WHQL PawnIO</div>
              </div>
              <span class="pill ${ram.ene_modules.length ? "emerald" : (ram.pawnio_status === "ready" ? "cyan" : "amber")}">
                <span class="dot"></span>${ram.ene_modules.length ? `${ram.ene_modules.length} DIMM RGB OK` : (ram.pawnio_status === "ready" ? "PawnIO Actif" : (ram.pawnio_status === "needs_admin" ? "PawnIO (Admin UAC)" : "PawnIO optionnel"))}
              </span>
            </div>

            ${ram.physical_dimms.length ? `
              <div class="stack" style="gap:6px;margin-bottom:12px">
                ${ram.physical_dimms.map((d) => `
                  <div class="row-between" style="padding:7px 10px;border-radius:8px;background:rgba(255,255,255,.03);border:1px solid var(--border)">
                    <div><strong>${esc(d.slot)}</strong> — ${esc(d.manufacturer)} <span class="mono">${esc(d.part_number)}</span></div>
                    <span class="muted mono">${d.capacity_gb} Go ${esc(d.memory_type)} • ${d.speed_mts} MT/s</span>
                  </div>
                `).join("")}
              </div>
            ` : ""}

            <div class="muted" style="margin-bottom:12px">${esc(ram.message)}</div>

            ${!ram.is_admin ? `
              <div class="row">
                <button class="btn btn-amber" id="ram-relaunch-admin">Relancer en Administrateur (UAC)</button>
                <button class="btn btn-secondary" id="ram-rescan">Rafraîchir</button>
              </div>
            ` : `
              ${ram.ene_modules.length ? `
                <div class="row" style="gap:8px;flex-wrap:wrap;margin-bottom:10px">
                  ${ram.ene_modules.map((m) => `
                    <span class="pill cyan"><span class="dot"></span>0x${m.address_7bit.toString(16).toUpperCase().padStart(2, "0")} • ${esc(m.firmware)} (${m.led_count} LEDs, ${m.is_v2 ? "V2" : "V1"})</span>
                  `).join("")}
                </div>
                <div class="seg-group" style="width:100%">
                  ${ramModes.map((m) => `<button class="seg-btn cyan ${ram.last_mode === m.id ? "active" : ""}" data-ram-mode="${m.id}">${esc(m.label)}</button>`).join("")}
                </div>
                <div class="row mt-12">
                  <span class="muted">Couleur :</span>
                  <input type="color" id="ram-color" class="og-color" value="${ram.last_color || "#00D2FF"}" />
                  <div class="swatch-row">
                    ${SWATCHES.map(([n, hex]) => `<button class="swatch ${(ram.last_color || "").toUpperCase() === hex ? "active" : ""}" data-ram-swatch="${hex}" style="background:${hex}" title="${n}"></button>`).join("")}
                  </div>
                </div>
                <div class="row-between mt-12" style="gap:12px">
                  <span class="muted">Vitesse effet ENE</span>
                  <input type="range" id="ram-speed" class="og-range" style="flex:1" min="0" max="4" value="${ram.last_speed ?? 2}" />
                  <span class="mono" id="ram-speed-lbl">${["Très rapide", "Rapide", "Normal", "Lent", "Très lent"][ram.last_speed ?? 2] || "Normal"}</span>
                </div>
                <div class="row mt-12">
                  <button class="btn btn-primary" id="ram-apply">Appliquer RAM en direct</button>
                  <button class="btn btn-rog" id="ram-sync-aura">Synchroniser avec couleur Aura</button>
                  <button class="btn btn-secondary" id="ram-save-eeprom">Enregistrer en EEPROM (boot)</button>
                  <button class="btn btn-secondary" id="ram-rescan">Rescanner</button>
                </div>
              ` : `
                <div class="row">
                  <button class="btn btn-primary" id="ram-rescan">Scanner le bus SMBus (100% lecture seule)</button>
                </div>
              `}
            `}
          </div>
        </div>
      </div>
    `;
  }

  function bindAsus() {
    const retry = document.getElementById("asus-retry");
    if (retry) { retry.onclick = () => runAction("reconnect_all", {}, "Reconnexion tentée"); return; }
    document.querySelectorAll(".mb-hotspot").forEach((h) => {
      h.onclick = () => { selectedAsusZone = Number(h.dataset.zone); render(); };
    });
    document.getElementById("asus-direct-toggle").onchange = (e) =>
      runAction("asus_set_direct", { enabled: e.target.checked }, e.target.checked ? "Mode Direct ASUS Aura activé" : "Retour au mode matériel ASUS Aura");
    let chosenMode = S.asus.hw_mode;
    document.querySelectorAll("[data-aura-mode]").forEach((b) => {
      b.onclick = () => {
        chosenMode = b.dataset.auraMode;
        runAction("asus_set_hw_mode", { mode: chosenMode, color: document.getElementById("asus-color").value, commit: false }, `Mode Aura : ${b.textContent}`);
      };
    });
    document.querySelectorAll("[data-asus-swatch]").forEach((s) => {
      s.onclick = () => {
        document.getElementById("asus-color").value = s.dataset.asusSwatch;
        runAction("asus_set_hw_mode", { mode: chosenMode, color: s.dataset.asusSwatch, commit: false }, `Couleur Aura ${s.dataset.asusSwatch}`);
      };
    });
    document.getElementById("asus-apply-hw").onclick = () =>
      runAction("asus_set_hw_mode", { mode: chosenMode, color: document.getElementById("asus-color").value, commit: false }, "Effet ASUS Aura appliqué");
    document.getElementById("asus-commit-hw").onclick = () =>
      runAction("asus_set_hw_mode", { mode: chosenMode, color: document.getElementById("asus-color").value, commit: true }, "Effet ASUS Aura sauvegardé dans la mémoire Flash !");
    document.getElementById("asus-apply-sd").onclick = () =>
      runAction("asus_set_shutdown_mode", {
        mode: document.getElementById("asus-sd-mode").value,
        color: document.getElementById("asus-sd-color").value,
      }, "Mode Shutdown / Stealth enregistré en EEPROM !");
    document.querySelectorAll("[data-argb-ch]").forEach((r) => {
      r.oninput = () => {
        const lbl = document.getElementById(`argb-lbl-${r.dataset.argbCh}`);
        if (lbl) lbl.textContent = `${r.value} LEDs`;
      };
      r.onchange = () => runAction("asus_set_led_count", { channel: Number(r.dataset.argbCh), count: Number(r.value) }, `Header ARGB calibré sur ${r.value} LEDs`);
    });

    // RAM DDR5 RGB (ENE SMBus via ASUSManagement WMI)
    const ramAdminBtn = document.getElementById("ram-relaunch-admin");
    if (ramAdminBtn) {
      ramAdminBtn.onclick = () => runAction("cooling_relaunch_admin", {}, "Relance en mode Administrateur...");
    }
    const ramScanBtn = document.getElementById("ram-rescan");
    if (ramScanBtn) {
      ramScanBtn.onclick = () => runAction("ram_scan", {}, "Scan SMBus lecture seule terminé");
    }
    let chosenRamMode = S.ram_rgb?.last_mode || "Static";
    const getRamSpeed = () => Number(document.getElementById("ram-speed")?.value ?? 2);
    const getRamColor = () => document.getElementById("ram-color")?.value || "#00D2FF";
    const ramSpeedEl = document.getElementById("ram-speed");
    if (ramSpeedEl) {
      const labels = ["Très rapide", "Rapide", "Normal", "Lent", "Très lent"];
      ramSpeedEl.oninput = () => {
        const l = document.getElementById("ram-speed-lbl");
        if (l) l.textContent = labels[Number(ramSpeedEl.value)] || "Normal";
      };
    }
    document.querySelectorAll("[data-ram-mode]").forEach((b) => {
      b.onclick = () => {
        chosenRamMode = b.dataset.ramMode;
        runAction("ram_apply_rgb", { mode: chosenRamMode, color: getRamColor(), speed: getRamSpeed(), save_eeprom: false }, `RAM DDR5 : ${b.textContent}`);
      };
    });
    document.querySelectorAll("[data-ram-swatch]").forEach((s) => {
      s.onclick = () => {
        const inp = document.getElementById("ram-color");
        if (inp) inp.value = s.dataset.ramSwatch;
        runAction("ram_apply_rgb", { mode: chosenRamMode, color: s.dataset.ramSwatch, speed: getRamSpeed(), save_eeprom: false }, `Couleur RAM ${s.dataset.ramSwatch}`);
      };
    });
    const ramApplyBtn = document.getElementById("ram-apply");
    if (ramApplyBtn) {
      ramApplyBtn.onclick = () => runAction("ram_apply_rgb", { mode: chosenRamMode, color: getRamColor(), speed: getRamSpeed(), save_eeprom: false }, "Effet RGB appliqué sur la RAM DDR5 !");
    }
    const ramSyncBtn = document.getElementById("ram-sync-aura");
    if (ramSyncBtn) {
      ramSyncBtn.onclick = () => {
        const auraCol = document.getElementById("asus-color")?.value || S.asus?.hw_color || "#FF1848";
        const inp = document.getElementById("ram-color");
        if (inp) inp.value = auraCol;
        runAction("ram_apply_rgb", { mode: "Static", color: auraCol, speed: getRamSpeed(), save_eeprom: false }, `RAM DDR5 synchronisée sur ${auraCol} !`);
      };
    }
    const ramSaveBtn = document.getElementById("ram-save-eeprom");
    if (ramSaveBtn) {
      ramSaveBtn.onclick = () => runAction("ram_apply_rgb", { mode: chosenRamMode, color: getRamColor(), speed: getRamSpeed(), save_eeprom: true }, "Effet RAM DDR5 enregistré dans l'EEPROM (persistant au démarrage) !");
    }
  }

  // ==================== 3. COOLING & BE QUIET! SILENT LOOP 3 ====================
  function viewCooling() {
    const c = S.cooling;
    const ch = c.channels[selectedCoolingIdx] || c.channels[0];
    const sensors = S.telemetry.sensors || {};
    const gpuTemp = S.telemetry.gpu?.temp_c;
    const argbChs = (S.asus?.channels || []).filter((x) => x.kind === "argb");
    const prof = S.config.profile;
    const sl3Ch = prof.silent_loop3_argb_channel ?? (argbChs[0]?.index ?? 1);

    const liveParts = [];
    if (sensors.cpu_temp_c != null) liveParts.push(`CPU ${Math.round(sensors.cpu_temp_c)}°C`);
    if (gpuTemp != null) liveParts.push(`GPU ${Math.round(gpuTemp)}°C`);
    if (sensors.aio_pump_rpm != null) liveParts.push(`Pompe ${sensors.aio_pump_rpm} RPM`);

    return `
      <div class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar amber"></div>
          <div>
            <div class="page-title">Refroidissement &amp; Watercooling — be quiet! Silent Loop 3</div>
            <div class="page-sub">Courbes PWM 8 points BIOS/EC (ASUSManagement WMI)  •  Pompe 6-pôles 3 500 RPM  •  Waterblock ARGB Gen2</div>
          </div>
        </div>
        <div class="header-actions">
          <span class="pill rog" id="cool-live-gpu"><span class="dot"></span>${liveParts.length ? liveParts.join(" • ") : "GPU —"}</span>
          <span class="pill ${c.wmi_available ? "emerald" : "amber"}"><span class="dot"></span>${c.wmi_available ? "ASUS WMI Actif" : (c.is_admin ? "Admin OK" : "Session Standard")}</span>
        </div>
      </div>

      ${!c.is_admin ? `
        <div class="card accent-amber" style="margin-bottom:14px">
          <div class="row-between">
            <div>
              <div style="font-weight:700;color:var(--amber);font-size:14px">Élévation Administrateur requise pour lire &amp; écrire les courbes PWM dans la carte mère ASUS</div>
              <div class="muted" style="margin-top:2px">Windows protège l'interface BIOS/EC <span class="mono">root\\wmi\\ASUSManagement</span>. Tu peux déjà éditer tes courbes ici ou relancer OpenGear en Administrateur en 1 clic.</div>
            </div>
            <button class="btn btn-amber" id="btn-uac-relaunch">Relancer en Administrateur (UAC)</button>
          </div>
        </div>
      ` : ""}

      <!-- LIVE SENSORS STRIP (LibreHardwareMonitor / OpenHardwareMonitor WMI) -->
      <div class="card" style="margin-bottom:14px">
        <div class="row-between">
          <div>
            <div class="card-title">Capteurs Temps Réel — RPM Pompe Silent Loop 3, Ventilateurs &amp; CPU Tdie</div>
            <div class="muted">${esc(sensors.source || "Sonde capteurs")}</div>
          </div>
          <span class="pill ${sensors.available ? "emerald" : "amber"}"><span class="dot"></span>${sensors.available ? "Télémétrie EC Live" : "LHM WMI optionnel"}</span>
        </div>
        ${sensors.available ? `
          <div class="sensor-chip-row">
            ${sensors.cpu_temp_c != null ? `<div class="sensor-chip">CPU Tdie <strong>${sensors.cpu_temp_c.toFixed(1)} °C</strong></div>` : ""}
            ${sensors.motherboard_temp_c != null ? `<div class="sensor-chip">Carte Mère <strong>${sensors.motherboard_temp_c.toFixed(1)} °C</strong></div>` : ""}
            ${sensors.aio_pump_rpm != null ? `<div class="sensor-chip" style="border-color:rgba(255,159,46,0.45)">AIO_PUMP (Silent Loop 3) <strong style="color:var(--amber)">${sensors.aio_pump_rpm} RPM</strong></div>` : ""}
            ${(sensors.fans || []).map((f) => `<div class="sensor-chip">${esc(f.name)} <strong>${f.rpm} RPM</strong></div>`).join("")}
          </div>
        ` : `
          <div class="muted" style="margin-top:8px">
            Astuce : Lance <strong>LibreHardwareMonitor</strong> (open-source, gratuit) en arrière-plan pour afficher instantanément ici la température <strong>Ryzen 5 7600X (Tctl/Tdie)</strong> ainsi que les <strong>RPM exacts de la pompe Silent Loop 3 et de chaque ventilateur</strong> — sans installer Armoury Crate !
          </div>
        `}
      </div>

      <div class="grid-stage-wide">
        <div class="stack">
          <div class="hero-stage amber-bg" style="padding:14px">
            <img class="stage-img" src="/assets/aio_silentloop3.jpg" alt="be quiet! Silent Loop 3" style="max-height:290px" />
          </div>
          <div class="card">
            <div class="card-head">
              <div class="card-title">Presets Pompe Silent Loop 3 (AIO_PUMP)</div>
              <span class="pill amber">≥ 45% sécurité</span>
            </div>
            <div class="muted" style="margin-bottom:10px">Pompe PWM 4-pin 6-pôles (jusqu'à 3 500 RPM). Plancher anti-cavitation garanti.</div>
            <div class="row">
              <button class="btn btn-secondary btn-sm" data-sl3-preset="ultra_silent">Ultra-Silence (45–95%)</button>
              <button class="btn btn-secondary btn-sm" data-sl3-preset="default">Équilibré (55–100%)</button>
              <button class="btn btn-secondary btn-sm" data-sl3-preset="max_perf">Performance Max (75–100%)</button>
            </div>
          </div>
          <div class="card">
            <div class="card-head">
              <div class="card-title">Éclairage Waterblock ARGB 5V Gen2</div>
            </div>
            ${argbChs.length ? `
              <div class="row">
                <select id="sl3-argb-sel" class="og-select" style="flex:1">
                  ${argbChs.map((x) => `<option value="${x.index}" ${x.index === sl3Ch ? "selected" : ""}>${esc(x.name)} (${x.led_count} LEDs)</option>`).join("")}
                </select>
                <input type="color" id="sl3-color" class="og-color" value="${sl3Color}" />
                <button class="btn btn-sm btn-primary" id="sl3-apply-col">Appliquer</button>
              </div>
              <div class="mt-12">
                <label class="switch amber">
                  <input type="checkbox" id="sl3-thermal-rgb" ${prof.silent_loop3_thermal_rgb ? "checked" : ""} />
                  <span class="switch-track"></span>
                  <span>Couleur réactive à la température (${sensors.cpu_temp_c != null ? "CPU Tdie" : "GPU"} : Cyan → Vert → Ambre → Rouge)</span>
                </label>
              </div>
            ` : `<div class="muted">Contrôleur ASUS Aura requis pour l'ARGB du waterblock.</div>`}
          </div>
        </div>

        <div class="card">
          <div class="row-between" style="margin-bottom:10px">
            <div class="seg-group">
              ${c.channels.map((x) => `<button class="seg-btn ${x.is_pump ? "amber" : ""} ${x.index === selectedCoolingIdx ? "active" : ""}" data-cool-ch="${x.index}">${esc(x.short_label)}</button>`).join("")}
            </div>
            ${c.wmi_available ? `<button class="btn btn-sm btn-secondary" id="btn-read-bios">Relire BIOS</button>` : ""}
          </div>
          <div class="row-between" style="margin-bottom:12px">
            <div style="font-weight:650">${esc(ch.label)}</div>
            <div class="seg-group">
              ${[["Silent","Silencieux"],["Standard","Standard"],["Turbo","Turbo"],["FullSpeed","100%"],["Manual","Manuel 8 pts"]].map(([id, lbl]) => `
                <button class="seg-btn ${ch.policy === id ? "active" : ""}" data-cool-pol="${id}">${lbl}</button>
              `).join("")}
            </div>
          </div>

          <!-- FANCONTROL-INSPIRED SMART SOURCE & ANTI-YOYO HYSTERESIS -->
          <div class="row-between" style="margin-bottom:12px;padding:9px 12px;border-radius:10px;background:rgba(255,255,255,.03);border:1px solid var(--border);gap:12px;flex-wrap:wrap">
            <div class="row" style="gap:8px">
              <span class="muted" style="font-weight:600">Source courbe :</span>
              <select id="cool-source-sel" class="og-select" style="width:205px">
                <option value="CpuBios" ${(ch.source || "CpuBios") === "CpuBios" ? "selected" : ""}>CPU (BIOS Q-Fan autonome)</option>
                <option value="GpuLive" ${ch.source === "GpuLive" ? "selected" : ""}>GPU RX 7900 XT (Live ADL2)</option>
                <option value="MixCpuGpu" ${ch.source === "MixCpuGpu" ? "selected" : ""}>Mix max(CPU, GPU) — FanControl</option>
              </select>
            </div>
            <div class="row" style="gap:8px">
              <span class="muted">Hystérésis anti-yoyo :</span>
              <input type="range" id="cool-hyst" class="og-range" style="width:95px" min="0" max="10" value="${ch.hysteresis_c ?? 3}" />
              <span class="mono" id="cool-hyst-lbl" style="width:36px">${ch.hysteresis_c ?? 3}°C</span>
            </div>
          </div>

          <div class="curve-wrap">
            <svg id="fan-curve-svg" class="curve-svg" viewBox="0 0 680 270"></svg>
          </div>

          <div class="curve-pts-strip" id="curve-pts-strip">
            ${ch.curve.map((pt, i) => `<div class="curve-pt-chip"><span>P${i + 1}</span><strong>${pt.temp_c}° → ${pt.duty_pct}%</strong></div>`).join("")}
          </div>

          <div class="row mt-16">
            <button class="btn ${c.is_admin ? "btn-emerald" : "btn-secondary"}" id="btn-apply-curve">Appliquer à la carte mère (ASUS WMI)</button>
            <button class="btn btn-secondary" id="btn-reset-curve">Réinitialiser</button>
          </div>
        </div>
      </div>

      <!-- DIRECT UNIVERSAL SUPERIO LIVE PWM CONTROL (PAWNIO LPCIO — WORKS ON ANY NUVOTON MOTHERBOARD!) -->
      ${(() => {
        const sio = c.superio || { detected: false, fans: [], diagnostic: "" };
        return `
          <div class="card accent-emerald mt-16">
            <div class="card-head">
              <div>
                <div class="card-title">Contrôle Direct SuperIO PWM &amp; RPM Temps Réel (PawnIO LpcIO — Universel)</div>
                <div class="muted">${esc(sio.diagnostic || "Pilote directement la puce SuperIO Nuvoton de la carte mère comme FanControl, avec vérification matérielle read-back.")}</div>
              </div>
              <div class="row">
                <span class="pill ${sio.detected ? "emerald" : "amber"}"><span class="dot"></span>${sio.detected ? esc(sio.chip_name) : (c.is_admin ? "PawnIO PnP absent" : "Admin UAC requis")}</span>
                ${!sio.detected ? `<button class="btn btn-sm btn-amber" id="btn-install-pawnio">Installer / Réparer PawnIO (WHQL)</button>` : ""}
                <button class="btn btn-sm btn-secondary" id="sio-refresh">Rafraîchir RPM</button>
              </div>
            </div>
            ${sio.detected && sio.fans.length ? `
              <div class="stack" style="gap:8px">
                ${sio.fans.map((f) => `
                  <div class="row-between" style="padding:8px 12px;border-radius:10px;background:rgba(255,255,255,.03);border:1px solid var(--border);gap:12px">
                    <div style="min-width:145px">
                      <strong>${esc(f.label)}</strong>
                      <div class="mono muted" style="font-size:11px">${f.is_manual ? "Mode Manuel Direct" : `Mode BIOS Auto (0x${f.mode_raw.toString(16).toUpperCase().padStart(2, "0")})`}</div>
                    </div>
                    <span class="pill ${f.rpm > 0 ? "amber" : ""}" style="min-width:96px;justify-content:center">${f.rpm > 0 ? `${f.rpm} RPM` : "0 RPM"}</span>
                    <input type="range" class="og-range" style="flex:1" min="0" max="100" value="${f.pwm_pct}" data-sio-slider="${f.index}" />
                    <span class="mono" id="sio-pct-${f.index}" style="width:42px;text-align:right">${f.pwm_pct}%</span>
                    <button class="btn btn-sm btn-primary" data-sio-apply="${f.index}">Appliquer</button>
                    <button class="btn btn-sm btn-secondary" data-sio-auto="${f.index}">Auto BIOS</button>
                  </div>
                `).join("")}
              </div>
            ` : ""}
          </div>
        `;
      })()}
    `;
  }

  function renderFanCurveSvg() {
    const svg = document.getElementById("fan-curve-svg");
    if (!svg || !S) return;
    const ch = S.cooling.channels[selectedCoolingIdx];
    const accent = ch.is_pump ? "#FF9F2E" : "#00D4FF";
    const padL = 44, padR = 18, padT = 18, padB = 30, W = 680, H = 270;
    const pw = W - padL - padR, ph = H - padT - padB;
    const tx = (t) => padL + ((t - 20) / 80) * pw;
    const dy = (d) => padT + (1 - d / 100) * ph;

    let html = `<defs><linearGradient id="cg" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="${accent}" stop-opacity="0.32"/><stop offset="100%" stop-color="${accent}" stop-opacity="0.02"/></linearGradient></defs>`;
    for (const d of [0, 25, 50, 75, 100]) {
      const y = dy(d);
      html += `<line x1="${padL}" y1="${y}" x2="${W - padR}" y2="${y}" stroke="rgba(255,255,255,0.06)"/>`;
      html += `<text x="${padL - 8}" y="${y + 4}" fill="#64718c" font-size="10.5" text-anchor="end" font-family="Consolas,monospace">${d}%</text>`;
    }
    for (let t = 20; t <= 100; t += 10) {
      const x = tx(t);
      html += `<line x1="${x}" y1="${padT}" x2="${x}" y2="${H - padB}" stroke="rgba(255,255,255,0.06)"/>`;
      html += `<text x="${x}" y="${H - 10}" fill="#64718c" font-size="10.5" text-anchor="middle" font-family="Consolas,monospace">${t}°C</text>`;
    }
    if (ch.is_pump) {
      const fy = dy(40);
      html += `<rect x="${padL}" y="${fy}" width="${pw}" height="${H - padB - fy}" fill="rgba(255,42,85,0.08)"/>`;
      html += `<line x1="${padL}" y1="${fy}" x2="${W - padR}" y2="${fy}" stroke="rgba(255,42,85,0.45)" stroke-dasharray="4 4"/>`;
      html += `<text x="${padL + 8}" y="${fy + 15}" fill="rgba(255,140,160,0.85)" font-size="10.5">Plancher sécurité pompe Silent Loop 3 (40% min)</text>`;
    }
    const liveT = S.telemetry.sensors?.cpu_temp_c ?? S.telemetry.gpu?.temp_c;
    const liveSrc = S.telemetry.sensors?.cpu_temp_c != null ? "CPU" : "GPU";
    if (liveT != null) {
      const lx = tx(Math.max(20, Math.min(100, liveT)));
      html += `<line x1="${lx}" y1="${padT}" x2="${lx}" y2="${H - padB}" stroke="#FF2A55" stroke-width="1.4" stroke-dasharray="3 3"/>`;
      html += `<text x="${lx + 5}" y="${padT + 12}" fill="#FF2A55" font-size="10.5" font-family="Consolas,monospace">${liveSrc} ${Math.round(liveT)}°C</text>`;
    }

    const pts = ch.curve.map((p) => [tx(p.temp_c), dy(p.duty_pct)]);
    const polyPts = [[padL, pts[0][1]], ...pts, [W - padR, pts[pts.length - 1][1]]];
    const lineD = polyPts.map((p, i) => `${i === 0 ? "M" : "L"}${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(" ");
    const areaD = `${lineD} L${W - padR},${H - padB} L${padL},${H - padB} Z`;
    html += `<path d="${areaD}" fill="url(#cg)"/>`;
    html += `<path d="${lineD}" fill="none" stroke="${accent}" stroke-width="2.6" stroke-linejoin="round"/>`;

    pts.forEach(([x, y], i) => {
      const p = ch.curve[i];
      const rpm = ch.is_pump ? ` (~${Math.round((p.duty_pct * 3500) / 100)} RPM)` : "";
      html += `<g class="curve-node" data-pt="${i}" style="cursor:grab">
        <circle cx="${x}" cy="${y}" r="12" fill="transparent"/>
        <circle cx="${x}" cy="${y}" r="6.5" fill="${accent}" stroke="#090b10" stroke-width="2"/>
        <title>Point #${i + 1}: ${p.temp_c}°C → ${p.duty_pct}%${rpm}</title>
      </g>`;
    });
    svg.innerHTML = html;

    let dragIdx = -1;
    const onMove = (e) => {
      if (dragIdx < 0) return;
      const r = svg.getBoundingClientRect();
      const sx = ((e.clientX - r.left) / r.width) * W;
      const sy = ((e.clientY - r.top) / r.height) * H;
      const minT = dragIdx === 0 ? 20 : ch.curve[dragIdx - 1].temp_c + 2;
      const maxT = dragIdx === 7 ? 98 : ch.curve[dragIdx + 1].temp_c - 2;
      const nt = Math.max(minT, Math.min(maxT, Math.round(20 + ((sx - padL) / pw) * 80)));
      const floor = ch.is_pump ? 40 : 0;
      const nd = Math.max(floor, Math.min(100, Math.round((1 - (sy - padT) / ph) * 100)));
      ch.curve[dragIdx] = { temp_c: nt, duty_pct: nd };
      ch.policy = "Manual";
      renderFanCurveSvg();
      const strip = document.getElementById("curve-pts-strip");
      if (strip) {
        strip.innerHTML = ch.curve.map((pt, i) => `<div class="curve-pt-chip"><span>P${i + 1}</span><strong>${pt.temp_c}° → ${pt.duty_pct}%</strong></div>`).join("");
      }
    };
    const onUp = () => {
      if (dragIdx >= 0) {
        dragIdx = -1;
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
      }
    };
    svg.querySelectorAll(".curve-node").forEach((g) => {
      g.onpointerdown = (e) => {
        e.preventDefault();
        dragIdx = Number(g.dataset.pt);
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
      };
    });
  }

  function bindCooling() {
    renderFanCurveSvg();
    const uac = document.getElementById("btn-uac-relaunch");
    if (uac) uac.onclick = () => runAction("cooling_relaunch_admin", {}, "Relance Administrateur (UAC) demandée…", false);
    const rb = document.getElementById("btn-read-bios");
    if (rb) rb.onclick = () => runAction("cooling_read_bios_curves", {}, "Courbes PWM relues depuis le BIOS ASUS !");
    document.querySelectorAll("[data-cool-ch]").forEach((b) => {
      b.onclick = () => { selectedCoolingIdx = Number(b.dataset.coolCh); render(); };
    });
    document.querySelectorAll("[data-cool-pol]").forEach((b) => {
      b.onclick = () => {
        S.cooling.channels[selectedCoolingIdx].policy = b.dataset.coolPol;
        render();
      };
    });
    document.querySelectorAll("[data-sl3-preset]").forEach((b) => {
      b.onclick = () => {
        selectedCoolingIdx = 0;
        runAction("cooling_sl3_preset", { preset: b.dataset.sl3Preset }, `Preset Silent Loop 3 '${b.textContent}' chargé`);
      };
    });
    const srcSel = document.getElementById("cool-source-sel");
    if (srcSel) {
      srcSel.onchange = () => {
        const ch = S.cooling.channels[selectedCoolingIdx];
        ch.source = srcSel.value;
        runAction("cooling_update_channel", { index: selectedCoolingIdx, source: ch.source, hysteresis_c: ch.hysteresis_c ?? 3, apply: false }, `Source ${ch.short_label} : ${srcSel.options[srcSel.selectedIndex].text}`);
      };
    }
    const hystEl = document.getElementById("cool-hyst");
    if (hystEl) {
      hystEl.oninput = () => { document.getElementById("cool-hyst-lbl").textContent = `${hystEl.value}°C`; };
      hystEl.onchange = () => {
        const ch = S.cooling.channels[selectedCoolingIdx];
        ch.hysteresis_c = Number(hystEl.value);
        runAction("cooling_update_channel", { index: selectedCoolingIdx, source: ch.source || "CpuBios", hysteresis_c: ch.hysteresis_c, apply: false }, `Hystérésis anti-yoyo réglée sur ${ch.hysteresis_c}°C`);
      };
    }
    document.getElementById("btn-apply-curve").onclick = () => {
      const ch = S.cooling.channels[selectedCoolingIdx];
      runAction("cooling_update_channel", { index: selectedCoolingIdx, policy: ch.policy, source: ch.source || "CpuBios", hysteresis_c: ch.hysteresis_c ?? 3, curve: ch.curve, apply: true }, `Courbe ${ch.short_label} appliquée au BIOS ASUS !`);
    };
    document.getElementById("btn-reset-curve").onclick = () =>
      runAction("cooling_sl3_preset", { preset: "default" }, "Courbe réinitialisée");
    const sioRef = document.getElementById("sio-refresh");
    if (sioRef) sioRef.onclick = () => runAction("superio_refresh", {}, "Lectures SuperIO RPM/PWM rafraîchies");
    const pioBtn = document.getElementById("btn-install-pawnio");
    if (pioBtn) pioBtn.onclick = async () => {
      toast("Téléchargement de l'installateur officiel PawnIO v2.2.0 (WHQL)…");
      const data = await runAction("pawnio_install", {}, null);
      if (data?.last_action_msg) toast(data.last_action_msg);
    };
    document.querySelectorAll("[data-sio-slider]").forEach((r) => {
      r.oninput = () => {
        const lbl = document.getElementById(`sio-pct-${r.dataset.sioSlider}`);
        if (lbl) lbl.textContent = `${r.value}%`;
      };
    });
    document.querySelectorAll("[data-sio-apply]").forEach((b) => {
      b.onclick = async () => {
        const idx = Number(b.dataset.sioApply);
        const val = Number(document.querySelector(`[data-sio-slider="${idx}"]`)?.value ?? 50);
        const data = await runAction("superio_set_pwm", { channel: idx, duty_pct: val }, null);
        if (data?.last_action_msg) toast(data.last_action_msg);
      };
    });
    document.querySelectorAll("[data-sio-auto]").forEach((b) => {
      b.onclick = async () => {
        const idx = Number(b.dataset.sioAuto);
        const data = await runAction("superio_restore_auto", { channel: idx }, null);
        if (data?.last_action_msg) toast(data.last_action_msg);
      };
    });
    const applyCol = document.getElementById("sl3-apply-col");
    if (applyCol) {
      applyCol.onclick = () => {
        sl3Color = document.getElementById("sl3-color").value;
        runAction("cooling_sl3_argb", {
          channel: Number(document.getElementById("sl3-argb-sel").value),
          color: sl3Color,
          thermal: false,
        }, `Couleur ${sl3Color} appliquée au waterblock Silent Loop 3`);
      };
    }
    const thRgb = document.getElementById("sl3-thermal-rgb");
    if (thRgb) {
      thRgb.onchange = (e) => runAction("cooling_sl3_argb", {
        channel: Number(document.getElementById("sl3-argb-sel").value),
        thermal: e.target.checked,
      }, e.target.checked ? "Waterblock Silent Loop 3 en mode thermique temps réel !" : "Mode thermique waterblock désactivé");
    }
  }

  // ==================== 4. CORSAIR BRAGI v2 UNIVERSAL (MULTI-DEVICE) ====================
  function renderKeyboardHtml(mode, kb) {
    const az = S.config.use_azerty_labels;
    const bindings = S.config.profile.bindings || {};
    const maxX = 18.8, maxY = 7.6;
    const photoUrl = getK70ImageUrl();
    const showPhoto = mode === "paint" && kbViewMode === "photo" && photoUrl;

    return `
      <div class="kb-chassis">
        <div class="kb-topbar">
          <div class="kb-brand-strip">
            <span>${esc((kb?.product_name || "CORSAIR K70 RGB TKL").toUpperCase())}</span>
            ${kb?.pollrate_hz ? `<span class="pill emerald"><span class="dot"></span>${kb.pollrate_hz} Hz AXON</span>` : ""}
            ${kb?.esports_switch_on ? `<span class="pill rog"><span class="dot"></span>MODE TOURNOI ACTIF</span>` : ""}
            ${kb?.rapid_trigger_supported ? `<span class="pill purple">Rapid Trigger</span>` : ""}
            ${kb?.flashtap_supported ? `<span class="pill cyan">FlashTap</span>` : ""}
          </div>
          <div class="row">
            ${mode === "paint" && photoUrl ? `
              <div class="seg-group">
                <button class="seg-btn ${kbViewMode === "interactive" ? "active" : ""}" data-kbview="interactive">Vue Interactive RGB</button>
                <button class="seg-btn ${kbViewMode === "photo" ? "active" : ""}" data-kbview="photo">Vue Studio HD</button>
              </div>
            ` : `<div class="muted">${mode === "paint" ? "Clique une touche pour peindre sa couleur" : "Clique une touche pour la remapper ou lui assigner une macro"}</div>`}
          </div>
        </div>
        ${showPhoto ? `
          <div style="padding:8px 0">
            <img class="kb-real-photo" src="${photoUrl}" alt="${esc(kb?.product_name || "Corsair K70 RGB TKL")}" />
          </div>
        ` : `
          <div class="kb-canvas-wrap">
            ${S.keys_layout.map((k) => {
              const left = (k.x / maxX) * 100;
              const top = (k.y / maxY) * 100;
              const width = ((k.w - 0.1) / maxX) * 100;
              const height = ((k.h - 0.12) / maxY) * 100;
              const label = az ? k.azerty : k.qwerty;
              const isSel = selectedKeyId === k.id;
              const isBound = !!bindings[k.id];
              return `<button class="kb-key ${isSel ? "selected" : ""} ${isBound ? "bound" : ""}" data-key-id="${esc(k.id)}" data-led="${k.led_index}" data-nx="${k.norm_x}" data-ny="${k.norm_y}" style="left:${left.toFixed(2)}%;top:${top.toFixed(2)}%;width:${width.toFixed(2)}%;height:${height.toFixed(2)}%" title="${esc(k.id)} (LED #${k.led_index})">${esc(label)}</button>`;
            }).join("")}
          </div>
        `}
      </div>
    `;
  }

  function viewCorsair() {
    const list = S.corsair_devices || [];
    const dev = getSelectedCorsair();
    if (!dev) {
      return `
        <div class="page-header">
          <div class="page-title-wrap"><div class="page-bar"></div><div><div class="page-title">Corsair — Moteur Universel Bragi v2</div><div class="page-sub">${esc(S.corsair_err || "Aucun périphérique détecté")}</div></div></div>
          <div class="header-actions"><button class="btn btn-primary" id="k70-retry">Reconnecter</button></div>
        </div>
        <div class="card"><div class="muted">Branche n'importe quel clavier, souris, casque ou contrôleur Corsair Bragi v2 : il sera détecté automatiquement par Plug &amp; Play.</div></div>
      `;
    }

    const deviceSwitcher = list.length > 1 ? `
      <div class="subtabs" style="margin-bottom:14px">
        ${list.map((d, idx) => `<button class="subtab ${idx === selectedCorsairIdx ? "active" : ""}" data-cdev="${idx}">${esc(d.product_name)} (${esc(d.kind_label)})</button>`).join("")}
      </div>
    ` : "";

    const header = `
      <div class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar"></div>
          <div>
            <div class="page-title">${esc(dev.product_name)}</div>
            <div class="page-sub">${esc(dev.kind_label)} • ${dev.vid}:${dev.pid} • FW ${esc(dev.firmware)} • Report ${dev.out_report_len}B${dev.storage_total_kb ? ` • Flash ${dev.storage_free_kb ?? "?"}/${dev.storage_total_kb} Ko` : ""}</div>
          </div>
        </div>
        <div class="header-actions">
          ${dev.battery_pct != null ? `<span class="pill emerald"><span class="dot"></span>${dev.battery_pct}%</span>` : ""}
          <span class="pill cyan"><span class="dot"></span>${dev.mode === "sw" ? "Mode Logiciel (RGB Live)" : "Mode Matériel (Onboard)"}</span>
        </div>
      </div>
      ${deviceSwitcher}
    `;

    // Adaptive inspector by probed Corsair device kind!
    if (dev.kind === "Mouse" && dev.mouse) {
      return header + viewCorsairMouse(dev);
    }
    if (dev.kind === "Headset" && dev.headset) {
      return header + viewCorsairHeadset(dev);
    }
    if (dev.kind === "CoolerHub" && dev.cooler) {
      return header + `
        <div class="card accent-cyan">
          <div class="card-title">Contrôleur / Watercooling Corsair Bragi</div>
          <table class="meta-table mt-12">
            <tr><td class="muted">Gamme Cooler (0x55)</td><td class="mono">${dev.cooler.product_line_id ?? "—"}</td></tr>
            <tr><td class="muted">Version Pompe (0x57)</td><td class="mono">${dev.cooler.pump_version_id ?? "—"}</td></tr>
            <tr><td class="muted">Taille Radiateur (0x58)</td><td class="mono">${dev.cooler.radiator_size ?? "—"}</td></tr>
          </table>
        </div>
      `;
    }

    // Keyboard view (default / K70 RGB TKL & any Bragi keyboard)
    const prof = S.config.profile;
    const mask = dev.winlock_shortcuts_mask ?? 3;
    const maxHz = dev.max_pollrate_hz || 8000;
    const pollRates = [125, 250, 500, 1000, 2000, 4000, 8000].filter((hz) => hz <= maxHz);

    return header + `
      ${renderKeyboardHtml("paint", dev)}

      <div class="grid-2 mt-16">
        <div class="stack">
          <div class="card accent-cyan">
            <div class="card-head">
              <div class="card-title">Performance AXON &amp; Matériel (Bragi v2)</div>
              ${dev.pollrate_hz ? `<span class="pill emerald">${dev.pollrate_hz} Hz</span>` : ""}
            </div>
            ${dev.pollrate_hz ? `
              <div class="muted" style="margin-bottom:6px">Fréquence de polling USB :</div>
              <div class="seg-group" style="width:100%">
                ${pollRates.map((hz) => `
                  <button class="seg-btn ${dev.pollrate_hz === hz ? "active" : ""}" data-k70-hz="${hz}">${hz >= 1000 ? (hz/1000) + "K Hz" : hz + " Hz"}</button>
                `).join("")}
              </div>
            ` : ""}

            <div class="row-between mt-12">
              <span class="muted">Luminosité Matérielle :</span>
              <div style="flex:1;margin:0 12px"><input type="range" id="k70-bright" class="og-range" min="0" max="100" value="${dev.hw_brightness_pct ?? 100}" /></div>
              <span class="mono" id="k70-bright-lbl" style="width:40px;text-align:right">${dev.hw_brightness_pct ?? 100}%</span>
            </div>

            <div class="row-between mt-12">
              <span class="muted">Mode Contrôleur :</span>
              <div class="seg-group">
                <button class="seg-btn ${dev.mode === "hw" ? "active" : ""}" data-k70-mode="hw">Matériel (Onboard)</button>
                <button class="seg-btn ${dev.mode === "sw" ? "active" : ""}" data-k70-mode="sw">Logiciel (OpenGear)</button>
              </div>
            </div>

            ${dev.active_hw_profile != null ? `
              <div class="row-between mt-12">
                <span class="muted">Profil embarqué MCU (<span class="mono">0x6E</span>) :</span>
                <div class="seg-group">
                  ${[0, 1, 2].map((i) => `<button class="seg-btn ${dev.active_hw_profile === i ? "active" : ""}" data-k70-hwprof="${i}">Profil #${i + 1}</button>`).join("")}
                </div>
              </div>
            ` : ""}
          </div>

          <div class="card">
            <div class="card-head">
              <div class="card-title">Contrôles MCU Avancés (Reversés via REA / iCUE5)</div>
              <span class="pill purple">BragiProtocols.dll</span>
            </div>
            ${dev.hw_winlock != null ? `
              <div class="row-between">
                <label class="switch">
                  <input type="checkbox" id="k70-hw-winlock" ${dev.hw_winlock ? "checked" : ""} />
                  <span class="switch-track"></span>
                  <span>WinLock Matériel MCU (<span class="mono">0x45</span>)</span>
                </label>
                ${dev.esports_switch_on != null ? `<span class="pill ${dev.esports_switch_on ? "rog" : ""}">Switch Tournoi : ${dev.esports_switch_on ? "ON" : "OFF"}</span>` : ""}
              </div>
              <div class="muted mt-12" style="margin-bottom:6px">Raccourcis bloqués quand WinLock est actif (<span class="mono">0x4A</span>) :</div>
              <div class="row">
                ${[[1,"Touche Win"],[2,"Alt + Tab"],[4,"Alt + F4"],[8,"Shift + Tab"]].map(([bit, lbl]) => `
                  <label class="mini-check"><input type="checkbox" class="wl-bit" data-bit="${bit}" ${(mask & bit) ? "checked" : ""} /><span>${lbl}</span></label>
                `).join("")}
              </div>
            ` : ""}

            ${dev.debounce_enabled != null ? `
              <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border)">
                <label class="switch">
                  <input type="checkbox" id="k70-deb-en" ${dev.debounce_enabled ? "checked" : ""} />
                  <span class="switch-track"></span>
                  <span>Debounce Switch (<span class="mono">0x09</span>)</span>
                </label>
                <div class="row">
                  ${(dev.debounce_timeout_ms ?? 30) > 5 ? `<button class="btn btn-sm btn-primary" id="k70-deb-boost" title="Baisser le debounce matériel à 2 ms pour réduire la latence">Boost Esports (2 ms • -${(dev.debounce_timeout_ms ?? 30) - 2} ms)</button>` : `<span class="pill emerald">Latence optimisée (${dev.debounce_timeout_ms} ms)</span>`}
                  <select id="k70-deb-ms" class="og-select" style="width:92px">
                    ${[0, 1, 2, 3, 5, 10, 15, 20, 30, 60].map((v) => `<option value="${v}" ${(dev.debounce_timeout_ms ?? 30) === v ? "selected" : ""}>${v} ms</option>`).join("")}
                  </select>
                </div>
              </div>
            ` : ""}

            <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border)">
              <span class="muted">Couleurs Indicateurs / Mode Tournoi :</span>
              <div class="row">
                <select id="k70-ind-sel" class="og-select" style="width:165px">
                  <option value="esports">Rétroéclairage Tournoi (0x77)</option>
                  <option value="winlock_on">LED WinLock Actif (0x5D)</option>
                  <option value="winlock_off">LED WinLock Inactif (0x5E)</option>
                  <option value="profile">LED Bouton Profil (0x49)</option>
                  <option value="brightness">LED Luminosité (0x5C)</option>
                </select>
                <input type="color" id="k70-ind-col" class="og-color" value="#FF1848" />
                <button class="btn btn-sm btn-secondary" id="k70-ind-apply">Appliquer</button>
              </div>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-head">
            <div class="card-title">Studio Éclairage RGB (193 LEDs • 11 effets iCUE)</div>
            <label class="switch">
              <input type="checkbox" id="k70-rgb-en" ${prof.enable_rgb ? "checked" : ""} />
              <span class="switch-track"></span>
              <span>RGB Live Actif</span>
            </label>
          </div>
          <div class="seg-group" style="width:100%">
            ${S.effects.map((e) => `<button class="seg-btn ${prof.rgb.kind === e.id ? "active" : ""}" data-k70-eff="${e.id}">${esc(e.label)}</button>`).join("")}
          </div>

          <div class="row mt-12">
            <span class="muted">Primaire :</span>
            <input type="color" id="k70-c1" class="og-color" value="${prof.rgb.primary}" />
            <span class="muted">Secondaire :</span>
            <input type="color" id="k70-c2" class="og-color" value="${prof.rgb.secondary}" />
            <label class="mini-check" style="margin-left:auto"><input type="checkbox" id="k70-rev" ${prof.rgb.reverse ? "checked" : ""} /><span>Inverser sens</span></label>
          </div>

          <div class="swatch-row mt-12">
            ${SWATCHES.map(([n, hex]) => `<button class="swatch ${prof.rgb.primary.toUpperCase() === hex ? "active" : ""}" data-k70-swatch="${hex}" style="background:${hex}" title="${n}"></button>`).join("")}
          </div>

          <div class="row-between mt-12">
            <span class="muted" style="width:90px">Vitesse</span>
            <input type="range" id="k70-spd" class="og-range" style="flex:1" min="0.2" max="4.5" step="0.1" value="${prof.rgb.speed}" />
            <span class="mono" style="width:42px;text-align:right">${prof.rgb.speed.toFixed(1)}x</span>
          </div>
          <div class="row-between mt-12">
            <span class="muted" style="width:90px">Luminosité SW</span>
            <input type="range" id="k70-swbr" class="og-range" style="flex:1" min="0" max="1" step="0.05" value="${prof.rgb.brightness}" />
            <span class="mono" style="width:42px;text-align:right">${Math.round(prof.rgb.brightness * 100)}%</span>
          </div>

          <div class="row-between mt-16" style="padding-top:12px;border-top:1px solid var(--border)">
            <div class="row">
              <span class="muted">Pinceau Par Touche :</span>
              <input type="color" id="k70-paint" class="og-color" value="${paintColor}" />
              <span class="pill cyan">${esc(selectedKeyId)}</span>
            </div>
            <button class="btn btn-sm btn-secondary" id="k70-clear-pk">Effacer peinture</button>
          </div>
        </div>
      </div>

      <!-- CORSAIR ONBOARD FLASH HWP ENGINE (REVERSED VIA REA + GHIDRA 12.2) -->
      ${dev.hwp_profiles?.length ? (() => {
        const hp = dev.hwp_profiles[0];
        const l = hp.lighting;
        const c1 = l?.colors?.[0] || "#1ECFDF";
        const c2 = l?.colors?.[1] || "#9B3CFF";
        return `
          <div class="card accent-cyan mt-16">
            <div class="card-head">
              <div>
                <div class="card-title">Mémoire Embarquée Flash HWP (8 Mo) — Autonome sans OpenGear / sans iCUE</div>
                <div class="muted">Profil Flash <span class="mono">${esc(hp.profile_file_id)}</span> • Éclairage <span class="mono">${esc(l?.file_id || "—")}</span> (${l?.led_count || 0} LEDs) • ${hp.assignment_count} actions • ${hp.property_count} propriétés MCU</div>
              </div>
              <div class="row">
                <span class="pill emerald">${esc(l?.kind_label || "Flash OK")}</span>
                <button class="btn btn-sm btn-secondary" id="k70-hwp-reload">Relire Flash</button>
              </div>
            </div>
            <div class="row">
              <span class="muted">Nom :</span>
              <input id="k70-hwp-name" class="og-input" style="width:170px" value="${esc(hp.name)}" />
              <span class="muted">Effet Flash :</span>
              <select id="k70-hwp-kind" class="og-select" style="width:210px">
                ${(S.hwp_lighting_kinds || []).map((k) => `<option value="${k.id}" ${l?.kind === k.id ? "selected" : ""}>${esc(k.label)}</option>`).join("")}
              </select>
              <input type="color" id="k70-hwp-c1" class="og-color" value="${c1}" title="Couleur primaire Flash" />
              <input type="color" id="k70-hwp-c2" class="og-color" value="${c2}" title="Couleur secondaire Flash" />
              <select id="k70-hwp-speed" class="og-select" style="width:105px">
                <option value="0" ${l?.speed === 0 ? "selected" : ""}>Lent</option>
                <option value="1" ${(l?.speed ?? 1) === 1 ? "selected" : ""}>Moyen</option>
                <option value="2" ${l?.speed === 2 ? "selected" : ""}>Rapide</option>
              </select>
              <select id="k70-hwp-dir" class="og-select" style="width:115px">
                <option value="0" ${l?.direction === 0 ? "selected" : ""}>Gauche</option>
                <option value="1" ${(l?.direction ?? 1) === 1 ? "selected" : ""}>Droite</option>
                <option value="2" ${l?.direction === 2 ? "selected" : ""}>Haut</option>
                <option value="3" ${l?.direction === 3 ? "selected" : ""}>Bas</option>
                <option value="4" ${l?.direction === 4 ? "selected" : ""}>Horaire</option>
                <option value="5" ${l?.direction === 5 ? "selected" : ""}>Anti-horaire</option>
              </select>
              <button class="btn btn-primary" id="k70-hwp-flash" style="margin-left:auto">Flasher dans le K70 (Backup + Vérif)</button>
            </div>
          </div>
        `;
      })() : ""}
    `;
  }

  function viewCorsairMouse(dev) {
    const m = dev.mouse;
    return `
      <div class="grid-2">
        <div class="card accent-cyan">
          <div class="card-head">
            <div class="card-title">Capteur Souris Corsair Bragi — 5 Paliers DPI (<span class="mono">0x18..0x1E</span>)</div>
            <span class="pill cyan">${m.current_dpi_x || m.dpi_stages[m.active_stage]} DPI</span>
          </div>
          <div class="stack" style="gap:8px">
            ${m.dpi_stages.map((dpi, i) => `
              <div class="row-between">
                <button class="btn btn-sm ${m.active_stage === i ? "btn-primary" : "btn-secondary"}" data-cm-stage="${i}">Palier #${i + 1}${m.active_stage === i ? " ★" : ""}</button>
                <input type="number" class="og-input" style="width:110px" min="100" max="30000" step="50" value="${dpi}" data-cm-dpi-val="${i}" />
                <button class="btn btn-sm btn-secondary" data-cm-dpi-save="${i}">Appliquer</button>
              </div>
            `).join("")}
          </div>
        </div>
        <div class="card">
          <div class="card-head"><div class="card-title">Polling, Lift Height &amp; Batterie</div></div>
          ${dev.pollrate_hz ? `
            <div class="muted" style="margin-bottom:6px">Polling Rate :</div>
            <div class="seg-group">
              ${[125, 250, 500, 1000, 2000].map((hz) => `<button class="seg-btn ${dev.pollrate_hz === hz ? "active" : ""}" data-k70-hz="${hz}">${hz} Hz</button>`).join("")}
            </div>
          ` : ""}
          ${m.lift_height != null ? `
            <div class="row-between mt-12">
              <span class="muted">Lift Height (<span class="mono">0x07</span>) :</span>
              <div class="seg-group">
                ${[[0,"Bas"],[1,"Moyen"],[2,"Élevé"]].map(([lvl, lbl]) => `<button class="seg-btn ${m.lift_height === lvl ? "active" : ""}" data-cm-lod="${lvl}">${lbl}</button>`).join("")}
              </div>
            </div>
          ` : ""}
        </div>
      </div>
    `;
  }

  function viewCorsairHeadset(dev) {
    const h = dev.headset;
    return `
      <div class="card accent-purple">
        <div class="card-head">
          <div class="card-title">Casque Corsair Bragi — Sidetone (<span class="mono">0x46/0x47</span>)</div>
          <span class="pill purple" id="chs-side-pill">${h.sidetone_pct ?? 0}%</span>
        </div>
        <div class="row">
          <input type="range" id="chs-sidetone" class="og-range" style="flex:1" min="0" max="100" value="${h.sidetone_pct ?? 0}" />
          <button class="btn btn-sm btn-secondary" id="chs-side-off">Off (0%)</button>
        </div>
      </div>
    `;
  }

  function bindCorsair() {
    const retry = document.getElementById("k70-retry");
    if (retry) { retry.onclick = () => runAction("reconnect_all", {}, "Reconnexion Corsair"); return; }
    document.querySelectorAll("[data-cdev]").forEach((b) => {
      b.onclick = () => { selectedCorsairIdx = Number(b.dataset.cdev); render(); };
    });
    document.querySelectorAll("[data-kbview]").forEach((b) => {
      b.onclick = () => { kbViewMode = b.dataset.kbview; render(); };
    });
    const dIdx = selectedCorsairIdx;
    document.querySelectorAll(".kb-key").forEach((k) => {
      k.onclick = () => {
        selectedKeyId = k.dataset.keyId;
        const pEl = document.getElementById("k70-paint");
        if (pEl) paintColor = pEl.value;
        if (S.config.profile.rgb.kind === "PerKey") {
          runAction("set_per_key_color", { led_index: Number(k.dataset.led), color: paintColor }, `Touche ${selectedKeyId} peinte en ${paintColor}`);
        } else {
          render();
        }
      };
    });
    document.querySelectorAll("[data-k70-hz]").forEach((b) => {
      b.onclick = () => runAction("corsair_set_pollrate", { device_index: dIdx, hz: Number(b.dataset.k70Hz) }, `Polling rate réglé sur ${b.dataset.k70Hz} Hz !`);
    });
    const br = document.getElementById("k70-bright");
    if (br) {
      br.oninput = () => { document.getElementById("k70-bright-lbl").textContent = `${br.value}%`; };
      br.onchange = () => runAction("corsair_set_brightness", { device_index: dIdx, percent: Number(br.value) }, `Luminosité : ${br.value}%`);
    }
    document.querySelectorAll("[data-k70-mode]").forEach((b) => {
      b.onclick = () => runAction("corsair_set_mode", { device_index: dIdx, mode: b.dataset.k70Mode }, `Mode basculé sur ${b.textContent}`);
    });
    document.querySelectorAll("[data-k70-hwprof]").forEach((b) => {
      b.onclick = () => runAction("corsair_set_hw_profile", { device_index: dIdx, index: Number(b.dataset.k70Hwprof) }, `Profil embarqué #${Number(b.dataset.k70Hwprof) + 1} activé`);
    });
    const wl = document.getElementById("k70-hw-winlock");
    if (wl) {
      wl.onchange = (e) => runAction("corsair_set_hw_winlock", { device_index: dIdx, enabled: e.target.checked }, `WinLock MCU : ${e.target.checked ? "Activé" : "Désactivé"}`);
    }
    document.querySelectorAll(".wl-bit").forEach((c) => {
      c.onchange = () => {
        let mask = 0;
        document.querySelectorAll(".wl-bit:checked").forEach((x) => { mask |= Number(x.dataset.bit); });
        runAction("corsair_set_winlock_shortcuts", { device_index: dIdx, mask }, `Masque raccourcis WinLock MCU : 0x${mask.toString(16).toUpperCase()}`);
      };
    });
    const debBoost = document.getElementById("k70-deb-boost");
    if (debBoost) {
      debBoost.onclick = () => runAction("corsair_set_hw_debounce", { device_index: selectedCorsairIdx, enabled: true, timeout_ms: 2 }, "Debounce matériel K70 boosté à 2 ms ! (Gain de latence immédiat)");
    }
    const debEn = document.getElementById("k70-deb-en");
    const debMs = document.getElementById("k70-deb-ms");
    if (debEn && debMs) {
      const pushDeb = () => runAction("corsair_set_debounce", { device_index: dIdx, enabled: debEn.checked, timeout_ms: Number(debMs.value) }, "Debounce matériel mis à jour");
      debEn.onchange = pushDeb;
      debMs.onchange = pushDeb;
    }
    const indBtn = document.getElementById("k70-ind-apply");
    if (indBtn) {
      indBtn.onclick = () => runAction("corsair_set_indicator_color", {
        device_index: dIdx,
        indicator: document.getElementById("k70-ind-sel").value,
        color: document.getElementById("k70-ind-col").value,
      }, "Couleur indicateur matériel appliquée !");
    }
    // Corsair Mouse bindings
    document.querySelectorAll("[data-cm-stage]").forEach((b) => {
      b.onclick = () => runAction("corsair_mouse_set_active_stage", { device_index: dIdx, stage: Number(b.dataset.cmStage) }, `Palier DPI #${Number(b.dataset.cmStage) + 1} activé`);
    });
    document.querySelectorAll("[data-cm-dpi-save]").forEach((b) => {
      b.onclick = () => {
        const st = Number(b.dataset.cmDpiSave);
        const val = Number(document.querySelector(`[data-cm-dpi-val="${st}"]`).value);
        runAction("corsair_mouse_set_dpi_stage", { device_index: dIdx, stage: st, dpi: val }, `Palier DPI #${st + 1} = ${val}`);
      };
    });
    document.querySelectorAll("[data-cm-lod]").forEach((b) => {
      b.onclick = () => runAction("corsair_mouse_set_lift_height", { device_index: dIdx, level: Number(b.dataset.cmLod) }, `Lift Height réglé sur ${b.textContent}`);
    });
    // Corsair Headset bindings
    const chs = document.getElementById("chs-sidetone");
    if (chs) {
      chs.oninput = () => { document.getElementById("chs-side-pill").textContent = `${chs.value}%`; };
      chs.onchange = () => runAction("corsair_headset_set_sidetone", { device_index: dIdx, percent: Number(chs.value), muted: Number(chs.value) === 0 }, `Sidetone Corsair : ${chs.value}%`);
      document.getElementById("chs-side-off").onclick = () => runAction("corsair_headset_set_sidetone", { device_index: dIdx, percent: 0, muted: true }, "Sidetone coupé");
    }

    // Keyboard RGB bindings
    const rgbEn = document.getElementById("k70-rgb-en");
    if (rgbEn) {
      rgbEn.onchange = (e) => runAction("set_rgb_config", { enable_rgb: e.target.checked }, e.target.checked ? "RGB logiciel activé" : "RGB logiciel coupé");
      document.querySelectorAll("[data-k70-eff]").forEach((b) => {
        b.onclick = () => runAction("set_rgb_config", { kind: b.dataset.k70Eff, enable_rgb: true }, `Effet : ${b.textContent}`);
      });
      const pushRgb = () => runAction("set_rgb_config", {
        primary: document.getElementById("k70-c1").value,
        secondary: document.getElementById("k70-c2").value,
        speed: parseFloat(document.getElementById("k70-spd").value),
        brightness: parseFloat(document.getElementById("k70-swbr").value),
        reverse: document.getElementById("k70-rev").checked,
      }, null, false);
      ["k70-c1", "k70-c2", "k70-spd", "k70-swbr", "k70-rev"].forEach((id) => {
        document.getElementById(id).oninput = pushRgb;
      });
      document.querySelectorAll("[data-k70-swatch]").forEach((s) => {
        s.onclick = () => {
          document.getElementById("k70-c1").value = s.dataset.k70Swatch;
          paintColor = s.dataset.k70Swatch;
          runAction("set_rgb_config", { primary: s.dataset.k70Swatch }, `Couleur ${s.dataset.k70Swatch}`);
        };
      });
      document.getElementById("k70-paint").oninput = (e) => { paintColor = e.target.value; };
      document.getElementById("k70-clear-pk").onclick = () => runAction("clear_per_key", {}, "Peinture par touche réinitialisée");
    }
    const hwpRel = document.getElementById("k70-hwp-reload");
    if (hwpRel) {
      hwpRel.onclick = () => runAction("corsair_reload_hwp", { device_index: dIdx }, "Profils Flash HWP relus depuis le clavier !");
    }
    const hwpFlash = document.getElementById("k70-hwp-flash");
    if (hwpFlash) {
      hwpFlash.onclick = () => runAction("corsair_flash_hwp_lighting", {
        device_index: dIdx,
        profile_index: 0,
        name: document.getElementById("k70-hwp-name").value,
        kind: document.getElementById("k70-hwp-kind").value,
        primary: document.getElementById("k70-hwp-c1").value,
        secondary: document.getElementById("k70-hwp-c2").value,
        speed: Number(document.getElementById("k70-hwp-speed").value),
        direction: Number(document.getElementById("k70-hwp-dir").value),
      }, "Effet matériel flashé dans la mémoire du K70 et vérifié ! Persiste même app fermée.");
    }
  }

  // ==================== 5. LOGITECH G UNIVERSAL MULTI-DEVICE ====================
  function viewLogitech() {
    const tabs = [];
    (S.logi_mice || []).forEach((m, i) => tabs.push({ kind: "mouse", index: i, label: m.name }));
    (S.logi_keyboards || []).forEach((k, i) => tabs.push({ kind: "keyboard", index: i, label: k.name }));
    (S.logi_headsets || []).forEach((h, i) => tabs.push({ kind: "headset", index: i, label: h.name }));

    return `
      <div class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar"></div>
          <div>
            <div class="page-title">Logitech G — Moteur Universel HID++ 2.0</div>
            <div class="page-sub">Multi-appareils Plug &amp; Play • Souris LIGHTSPEED/Bolt, Claviers RGB (0x8071) &amp; Casques PRO X • Modèles batterie.xml officiels</div>
          </div>
        </div>
        <div class="header-actions">
          ${tabs.length ? `
            <div class="subtabs" style="margin:0">
              ${tabs.map((t) => `<button class="subtab ${logiSelection.kind === t.kind && logiSelection.index === t.index ? "active" : ""}" data-lkind="${t.kind}" data-lidx="${t.index}">${esc(t.label)}</button>`).join("")}
            </div>
          ` : ""}
        </div>
      </div>
      ${logiSelection.kind === "mouse" ? viewLogiMouse() : (logiSelection.kind === "keyboard" ? viewLogiKeyboard() : viewLogiHeadset())}
    `;
  }

  function viewLogiMouse() {
    const m = getSelectedLogiMouse();
    if (!m) {
      return `<div class="card"><div class="row-between"><span class="muted">Souris Logitech non détectée (${esc(S.logi_mouse_err || "en veille ?")})</span><button class="btn btn-primary" id="logi-retry">Réveiller &amp; Reconnecter</button></div></div>`;
    }
    const bat = m.battery;
    const pm = m.power_model;
    const dpi = m.dpi;
    const rr = m.report_rate;
    const ob = m.onboard;
    const p = draftProfile || m.onboard_profiles[0];
    const btns = p?.buttons || [];
    const btnCount = Math.max(btns.length, ob?.button_count || 5);
    const imgInfo = getMouseImageUrl();

    // Project exact G HUB metadata.json marker coordinates onto the centered mouse stage when official PNG is loaded!
    // Mouse image height is 350px inside 420px stage (aspect ratio ~0.497 -> width ~174px centered in ~460px container => ~38% width, 83% height)
    const markers = S.local_assets?.mouse_markers || [];
    const projectMarker = (idx, fallbackX, fallbackY) => {
      if (!imgInfo.official || !markers[idx]) return { px: fallbackX, py: fallbackY };
      const mk = markers[idx];
      // Image occupies roughly x:[31%..69%], y:[8%..92%] in the 460x420 mouse stage
      return {
        px: Number((31 + (mk.x_pct / 100) * 38).toFixed(1)),
        py: Number((8 + (mk.y_pct / 100) * 84).toFixed(1)),
      };
    };

    const callouts = [
      { idx: 0, id: "M1 • GAUCHE", ...projectMarker(0, 42, 28), lx: 2, ly: 14 },
      { idx: 1, id: "M2 • DROIT", ...projectMarker(1, 58, 28), lx: 73, ly: 14 },
      { idx: 2, id: "M3 • MOLETTE", ...projectMarker(2, 50, 24), lx: 73, ly: 39 },
      { idx: 4, id: "M5 • AVANT", ...projectMarker(4, 34, 41), lx: 2, ly: 41 },
      { idx: 3, id: "M4 • ARRIÈRE", ...projectMarker(3, 34, 53), lx: 2, ly: 66 },
    ].filter((c) => c.idx < btnCount);

    const defaultBtnNames = ["M1 • Clic Gauche", "M2 • Clic Droit", "M3 • Molette", "M4 • Arrière", "M5 • Avant"];

    return `
      <div class="grid-stage-wide">
        <div class="stack">
          <div class="hero-stage">
            <div class="mouse-stage-inner">
              <img src="${imgInfo.url}" class="${imgInfo.official ? "official-cutout" : ""}" alt="${esc(m.name)}" />
              <svg class="mouse-svg-overlay" viewBox="0 0 100 100" preserveAspectRatio="none">
                ${callouts.map((c) => {
                  const sel = selectedMouseBtnIdx === c.idx;
                  const endX = c.lx < 50 ? c.lx + 24 : c.lx;
                  const endY = c.ly + 6;
                  return `
                    <line x1="${c.px}" y1="${c.py}" x2="${endX}" y2="${endY}" stroke="${sel ? "#00D4FF" : "rgba(255,255,255,0.28)"}" stroke-width="${sel ? 0.6 : 0.32}" vector-effect="non-scaling-stroke"/>
                    <circle cx="${c.px}" cy="${c.py}" r="1.25" fill="${sel ? "#00D4FF" : "#cfd8ea"}"/>
                  `;
                }).join("")}
              </svg>
              ${callouts.map((c) => {
                const sel = selectedMouseBtnIdx === c.idx;
                const lbl = btns[c.idx]?.label || "Standard";
                return `
                  <button class="callout-pin ${sel ? "active" : ""}" data-mbtn="${c.idx}" style="left:${c.lx}%;top:${c.ly}%">
                    <div class="pin-id">${c.id}</div>
                    <div class="pin-act">${esc(lbl)}</div>
                  </button>
                `;
              }).join("")}
              ${imgInfo.variants.length > 1 ? `
                <div class="variant-bar">
                  ${imgInfo.variants.map((v, vi) => `<button class="variant-chip ${vi === selectedMouseVariant ? "active" : ""}" data-mvar="${vi}">${esc(v.label.replace(" (Officiel)", ""))}</button>`).join("")}
                </div>
              ` : ""}
            </div>
          </div>

          <!-- EXACT G HUB BATTERY POWER MODEL -->
          <div class="card">
            <div class="card-head">
              <div class="card-title">Batterie &amp; Modèle Énergétique (battery.xml)</div>
              ${bat ? `<span class="pill emerald"><span class="dot"></span>${bat.percent ?? "?"}% • ${esc(bat.state)}</span>` : ""}
            </div>
            ${pm ? `
              <div class="row-between">
                <div>
                  <div style="font-size:26px;font-weight:750;color:var(--emerald);letter-spacing:-0.03em">~${pm.estimated_hours.toFixed(1)} h</div>
                  <div class="muted">Autonomie estimée à ${rr?.current_hz || 1000} Hz (${pm.total_mw.toFixed(2)} mW total • ${Math.round(pm.capacity_mwh)} mWh)</div>
                </div>
                <div style="text-align:right;font-size:11px;color:var(--text-2)" class="mono">
                  Capteur : ${pm.sensor_mw.toFixed(2)} mW<br/>
                  MCU : ${pm.mcu_mw.toFixed(2)} mW<br/>
                  Radio : ${pm.radio_mw.toFixed(2)} mW
                </div>
              </div>
              <div class="bar-track"><div class="bar-fill" style="width:${bat?.percent ?? 0}%;background:var(--emerald)"></div></div>
            ` : `<div class="muted">Batterie indisponible</div>`}
            <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border);font-size:11.5px">
              <span class="mono muted">${esc(m.firmware.join(" | "))}</span>
              <span class="mono muted">HID++ v${esc(m.proto_version)} • ${m.features.length} features</span>
            </div>
          </div>
        </div>

        <div class="stack">
          <div class="card accent-cyan">
            <div class="card-head">
              <div class="card-title">Capteur Optique &amp; Fréquence LIGHTSPEED</div>
              <span class="pill cyan">${dpi ? `${dpi.dpi_x}${dpi.supports_independent_xy && dpi.dpi_y !== dpi.dpi_x ? "×" + dpi.dpi_y : ""} DPI` : "—"} • ${rr?.current_hz || 1000} Hz</span>
            </div>
            ${dpi ? `
              <div class="row-between">
                <span class="muted">${dpi.supports_independent_xy ? "DPI X :" : "Sensibilité DPI :"}</span>
                <div style="flex:1;margin:0 12px">
                  <input type="range" id="logi-dpi-slider" class="og-range" min="${dpi.min_dpi}" max="${dpi.max_dpi}" step="${dpi.step_dpi}" value="${dpi.dpi_x}" />
                </div>
                <span class="mono" id="logi-dpi-lbl" style="width:74px;text-align:right;font-weight:700;color:var(--cyan)">${dpi.dpi_x} DPI</span>
              </div>
              ${dpi.supports_independent_xy ? `
                <div class="row-between mt-12">
                  <span class="muted">DPI Y :</span>
                  <div style="flex:1;margin:0 12px">
                    <input type="range" id="logi-dpiy-slider" class="og-range" min="${dpi.min_dpi}" max="${dpi.max_dpi}" step="${dpi.step_dpi}" value="${dpi.dpi_y}" />
                  </div>
                  <span class="mono" id="logi-dpiy-lbl" style="width:74px;text-align:right;font-weight:700;color:var(--cyan)">${dpi.dpi_y} DPI</span>
                </div>
              ` : ""}
              <div class="row mt-12">
                ${[400, 800, 1200, 1600, 3200, 6400].filter((v) => v <= dpi.max_dpi).map((v) => `
                  <button class="btn btn-sm ${dpi.dpi_x === v ? "btn-primary" : "btn-secondary"}" data-logi-dpi="${v}">${v}</button>
                `).join("")}
                <button class="btn btn-sm btn-primary" id="logi-dpi-apply" style="margin-left:auto">Appliquer DPI</button>
              </div>
              ${dpi.supports_lod ? `
                <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border)">
                  <span class="muted">Distance de décrochage (LOD HERO 2) :</span>
                  <div class="seg-group">
                    ${["Low", "Medium", "High"].map((l) => `<button class="seg-btn ${dpi.lod === l ? "active" : ""}" data-logi-lod="${l}">${l}</button>`).join("")}
                  </div>
                </div>
              ` : ""}
            ` : ""}
            ${rr ? `
              <div class="row-between mt-12" style="padding-top:12px;border-top:1px solid var(--border)">
                <span class="muted">Polling Rate :</span>
                <div class="seg-group">
                  ${rr.supported_hz.map((hz) => `<button class="seg-btn ${rr.current_hz === hz ? "active" : ""}" data-logi-hz="${hz}">${hz} Hz</button>`).join("")}
                </div>
              </div>
            ` : ""}
            ${m.angle_snapping != null ? `
              <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border)">
                <label class="switch">
                  <input type="checkbox" id="logi-asnap" ${m.angle_snapping ? "checked" : ""} />
                  <span class="switch-track"></span>
                  <span>Angle Snapping matériel (<span class="mono">0x2230</span>)</span>
                </label>
              </div>
            ` : ""}
          </div>

          <!-- ONBOARD MEMORY (0x8100) + HARDWARE MACRO FLASH -->
          ${ob && p ? `
            <div class="card">
              <div class="card-head">
                <div class="card-title">Mémoire Embarquée (<span class="mono">0x8100</span>) — ${m.onboard_profiles.length} Profils Flash</div>
                <div class="seg-group">
                  <button class="seg-btn ${ob.mode === "Onboard" ? "active" : ""}" data-logi-mode="Onboard">Onboard (Flash)</button>
                  <button class="seg-btn ${ob.mode === "Host" ? "active" : ""}" data-logi-mode="Host">Host (Direct)</button>
                </div>
              </div>

              <div class="row-between" style="margin-bottom:10px">
                <div class="seg-group">
                  ${m.onboard_profiles.map((pr, idx) => `
                    <button class="seg-btn ${idx === selectedMouseProfileIdx ? "active" : ""}" data-logi-prof="${idx}">
                      #${pr.slot_index}${pr.slot_index === ob.active_profile ? " ★" : ""}
                    </button>
                  `).join("")}
                </div>
                <div class="row">
                  <span class="pill ${p.crc_valid ? "emerald" : "amber"}">${p.from_rom ? "ROM" : "FLASH"} ${p.sector_addr} • CRC OK</span>
                  <button class="btn btn-sm btn-secondary" id="logi-activate-prof">Activer sur souris</button>
                </div>
              </div>

              <div class="grid-2" style="gap:12px">
                <div>
                  <div class="muted" style="margin-bottom:6px;font-weight:600">5 Paliers DPI embarqués (défaut = radio) :</div>
                  <div class="stack" style="gap:6px">
                    ${p.dpi_stages.map((d, st) => `
                      <div class="row" style="gap:6px">
                        <input type="radio" name="def-dpi" value="${st}" ${p.default_dpi_idx === st ? "checked" : ""} style="accent-color:var(--cyan)" />
                        <span class="muted" style="width:52px">Palier ${st + 1}</span>
                        <input type="number" class="og-input" style="width:86px;padding:4px 7px" min="100" max="32000" step="50" value="${d}" data-stage-dpi="${st}" />
                        <button class="btn btn-sm btn-secondary" data-test-stage="${st}">Tester</button>
                      </div>
                    `).join("")}
                  </div>
                </div>
                <div>
                  <div class="muted" style="margin-bottom:6px;font-weight:600">Remapping des ${btnCount} boutons (en Flash) :</div>
                  <div class="stack" style="gap:6px;max-height:210px;overflow-y:auto">
                    ${Array.from({ length: btnCount }, (_, bIdx) => {
                      const name = defaultBtnNames[bIdx] || `Bouton M${bIdx + 1}`;
                      const curHex = btns[bIdx]?.hex || "";
                      const curLbl = btns[bIdx]?.label || "Standard";
                      const hasPreset = S.button_presets.some((bp) => bp.hex === curHex);
                      return `
                        <div class="row" style="gap:6px;${selectedMouseBtnIdx === bIdx ? "background:rgba(0,212,255,0.09);padding:3px 5px;border-radius:6px" : ""}">
                          <span style="width:94px;font-size:11.5px;color:${selectedMouseBtnIdx === bIdx ? "var(--cyan)" : "var(--text-2)"}">${name}</span>
                          <select class="og-select" style="flex:1;padding:4px 7px;font-size:11.5px" data-btn-remap="${bIdx}">
                            ${!hasPreset ? `<option value="${curHex}" selected>${esc(curLbl)}</option>` : ""}
                            ${S.button_presets.map((bp) => `<option value="${bp.hex}" ${bp.hex === curHex ? "selected" : ""}>${esc(bp.label)}</option>`).join("")}
                          </select>
                        </div>
                      `;
                    }).join("")}
                  </div>
                </div>
              </div>

              <div class="row-between mt-16" style="padding-top:12px;border-top:1px solid var(--border)">
                <div class="row">
                  <span class="muted">Polling profil :</span>
                  <div class="seg-group">
                    ${[125, 250, 500, 1000].map((hz) => `<button class="seg-btn ${p.report_rate_hz === hz ? "active" : ""}" data-prof-hz="${hz}">${hz}Hz</button>`).join("")}
                  </div>
                </div>
                <div class="row">
                  <button class="btn btn-secondary btn-sm" id="logi-reload-flash">Recharger</button>
                  <button class="btn btn-primary" id="logi-flash-btn">Flasher dans la souris (CRC-16 + Backup)</button>
                </div>
              </div>

              <!-- ONBOARD HARDWARE MACRO SECTOR FLASHER (0x8100 sector 0x0006+) -->
              <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border)">
                <div class="row">
                  <span class="muted">Macro Flash (<span class="mono">0x8100</span>) sur bouton <strong>M${selectedMouseBtnIdx + 1}</strong> :</span>
                  <select id="logi-macro-preset" class="og-select" style="width:190px">
                    <option value="double_click">Double Clic Gauche (15ms)</option>
                    <option value="rapid_e">Double Touche E (20ms)</option>
                    <option value="jump_crouch">Saut + Accroupir (Espace+Ctrl)</option>
                  </select>
                </div>
                <button class="btn btn-sm btn-secondary" id="logi-flash-macro-btn">Flasher Macro dans M${selectedMouseBtnIdx + 1}</button>
              </div>
            </div>
          ` : ""}
        </div>
      </div>
    `;
  }

  function viewLogiKeyboard() {
    const kb = getSelectedLogiKeyboard();
    if (!kb) return `<div class="card"><div class="muted">Aucun clavier Logitech HID++ détecté.</div></div>`;
    return `
      <div class="grid-2">
        <div class="card accent-cyan">
          <div class="card-head">
            <div class="card-title">${esc(kb.name)} — Éclairage RGB (<span class="mono">0x8071</span>)</div>
            ${kb.battery?.percent != null ? `<span class="pill emerald">${kb.battery.percent}%</span>` : ""}
          </div>
          <div class="row">
            <select id="lkb-mode" class="og-select" style="width:160px">
              <option value="Fixed">Statique</option>
              <option value="Breathing">Respiration</option>
              <option value="Cycle">Cycle Spectre</option>
              <option value="Wave">Vague Arc-en-ciel</option>
              <option value="Off">Éteint</option>
            </select>
            <input type="color" id="lkb-col" class="og-color" value="#00D4FF" />
            <button class="btn btn-primary btn-sm" id="lkb-apply-rgb">Appliquer RGB</button>
          </div>
          ${kb.supports_brightness ? `
            <div class="row-between mt-12">
              <span class="muted">Luminosité (<span class="mono">0x8040</span>) :</span>
              <input type="range" id="lkb-br" class="og-range" style="flex:1;margin:0 12px" min="0" max="100" value="${kb.brightness_pct ?? 100}" />
              <span class="mono" id="lkb-br-lbl">${kb.brightness_pct ?? 100}%</span>
            </div>
          ` : ""}
        </div>
        <div class="card">
          <div class="card-title">Informations Matérielles</div>
          <table class="meta-table mt-12">
            <tr><td class="muted">VID:PID</td><td class="mono">${kb.vid}:${kb.pid} (slot ${kb.device_index})</td></tr>
            <tr><td class="muted">Protocole</td><td class="mono">HID++ v${esc(kb.proto_version)}</td></tr>
            <tr><td class="muted">Firmware</td><td class="mono">${esc(kb.firmware.join(" | ") || "—")}</td></tr>
          </table>
        </div>
      </div>
    `;
  }

  function viewLogiHeadset() {
    const h = getSelectedLogiHeadset();
    if (!h) {
      return `<div class="card"><div class="row-between"><span class="muted">Casque Logitech PRO X non détecté (${esc(S.logi_headset_err || "absent")})</span><button class="btn btn-primary" id="logi-retry">Reconnecter</button></div></div>`;
    }
    const eq = h.equalizer;
    const freqs = eq?.frequencies_hz || [114, 398, 1000, 4999, 9999];
    const fmtF = (f) => (f >= 1000 ? `${(f / 1000).toFixed(f % 1000 ? 1 : 0)} kHz` : `${f} Hz`);
    const imgInfo = getHeadsetImageUrl();
    const bvPresets = S.local_assets?.blue_voice_presets || [];

    return `
      <div class="grid-stage">
        <div class="stack">
          <div class="hero-stage purple-bg" style="padding:16px">
            <img class="stage-img ${imgInfo.official ? "official-cutout" : ""}" src="${imgInfo.url}" alt="${esc(h.name)}" style="max-height:310px" />
            ${imgInfo.variants.length > 1 ? `
              <div class="variant-bar">
                ${imgInfo.variants.map((v, vi) => `<button class="variant-chip ${vi === selectedHeadsetVariant ? "active" : ""}" data-hvar="${vi}">${esc(v.label.replace("PRO X ", ""))}</button>`).join("")}
              </div>
            ` : ""}
          </div>
          <div class="card accent-purple">
            <div class="card-head">
              <div class="card-title">Sidetone — Retour Micro DAC (<span class="mono">0x8300</span>)</div>
              <span class="pill purple" id="hs-side-pill">${h.sidetone_pct}%</span>
            </div>
            <div class="muted" style="margin-bottom:10px">Monitoring matériel zéro-latence directement dans le DAC USB PRO X.</div>
            <div class="row">
              <input type="range" id="hs-sidetone" class="og-range" style="flex:1" min="0" max="100" value="${h.sidetone_pct}" />
              <button class="btn btn-sm btn-secondary" id="hs-side-off">Off (0%)</button>
            </div>
            <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border);font-size:11.5px">
              <span class="mono muted">${esc(h.firmware.join(" | "))}</span>
              <span class="mono muted">${h.vid}:${h.pid} • HID++ v${esc(h.proto_version)}</span>
            </div>
          </div>
        </div>

        <div class="stack">
          <div class="card">
            <div class="card-head">
              <div class="card-title">Égaliseur Matériel DSP 5 Bandes (<span class="mono">0x8310</span>)</div>
              <span class="pill purple">Presets Pro officiels G HUB</span>
            </div>

            <div class="row" style="margin-bottom:12px">
              ${h.presets.map((pr, idx) => `<button class="btn btn-sm btn-secondary" data-eq-preset="${idx}">${esc(pr.name)}</button>`).join("")}
            </div>

            <div class="eq-console">
              <svg id="eq-curve-svg" class="eq-graph" viewBox="0 0 560 112"></svg>
              <div class="eq-faders">
                ${freqs.map((f, i) => `
                  <div class="eq-band">
                    <span class="eq-db" id="eq-db-${i}">${eqDraft[i] > 0 ? "+" : ""}${eqDraft[i]} dB</span>
                    <div class="eq-vslider-wrap">
                      <input type="range" class="eq-vslider" data-eq-band="${i}" min="-12" max="12" step="1" value="${eqDraft[i]}" />
                    </div>
                    <span class="eq-freq">${fmtF(f)}</span>
                  </div>
                `).join("")}
              </div>
            </div>

            <div class="row mt-16">
              <button class="btn btn-primary" id="hs-apply-eq">Appliquer l'égaliseur au DAC PRO X</button>
              <button class="btn btn-secondary" id="hs-reset-eq">Remettre à plat (0 dB)</button>
            </div>
          </div>

          <!-- NATIVE WINDOWS CORE AUDIO HUB (WASAPI — ZERO BLOAT) -->
          ${(() => {
            const au = S.telemetry?.audio || { available: false, output_volume_pct: 50, output_muted: false, output_peak: 0, mic_available: false, mic_volume_pct: 80, mic_muted: false };
            return `
              <div class="card accent-cyan">
                <div class="card-head">
                  <div class="card-title">Contrôle Audio Windows Natif (WASAPI)</div>
                  <span class="pill cyan">Volume &amp; Micro Live</span>
                </div>
                <div class="stack" style="gap:10px">
                  <div class="row-between" style="gap:10px">
                    <span class="muted" style="width:115px">Sortie Master</span>
                    <input type="range" id="wa-out-vol" class="og-range" style="flex:1" min="0" max="100" value="${au.output_volume_pct}" />
                    <span class="mono" id="wa-out-lbl" style="width:42px;text-align:right">${au.output_volume_pct}%</span>
                    <button class="btn btn-sm ${au.output_muted ? "btn-rog" : "btn-secondary"}" id="wa-out-mute">${au.output_muted ? "Muet" : "Mute"}</button>
                  </div>
                  ${au.mic_available ? `
                    <div class="row-between" style="gap:10px">
                      <span class="muted" style="width:115px">Microphone</span>
                      <input type="range" id="wa-mic-vol" class="og-range" style="flex:1" min="0" max="100" value="${au.mic_volume_pct}" />
                      <span class="mono" id="wa-mic-lbl" style="width:42px;text-align:right">${au.mic_volume_pct}%</span>
                      <button class="btn btn-sm ${au.mic_muted ? "btn-rog" : "btn-secondary"}" id="wa-mic-mute">${au.mic_muted ? "Micro Coupé" : "Mute Micro"}</button>
                    </div>
                  ` : ""}
                </div>
              </div>
            `;
          })()}

          <!-- HONEST BLUE VO!CE / APO ARCHITECTURE STATUS -->
          <div class="card">
            <div class="card-head">
              <div class="card-title">Blue VO!CE &amp; DTS Headphone:X (Windows Audio APO)</div>
              <span class="pill amber">DSP Logiciel Windows</span>
            </div>
            <div class="muted">
              Contrairement au Sidetone (<span class="mono">0x8300</span>) et à l'EQ 5 bandes (<span class="mono">0x8310</span>) qui tournent <strong>dans le DAC USB matériel</strong>, les filtres micro Blue VO!CE et DTS 7.1 sont exécutés côté Windows par le pilote signé <span class="mono">ghub_audio_driver_apo</span>.
              ${bvPresets.length ? `OpenGear a bien détecté <strong>${bvPresets.length} presets Blue VO!CE</strong> dans ton dépôt local (${esc(bvPresets.slice(0, 5).join(", "))}…).` : ""}
            </div>
          </div>
        </div>
      </div>
    `;
  }

  function renderEqSvg() {
    const svg = document.getElementById("eq-curve-svg");
    if (!svg) return;
    const W = 560, H = 112, padX = 48, midY = H / 2;
    const xs = [0, 1, 2, 3, 4].map((i) => padX + (i / 4) * (W - padX * 2));
    const ys = eqDraft.map((db) => midY - (db / 12) * (H * 0.38));
    let d = `M ${xs[0]},${ys[0]}`;
    for (let i = 0; i < 4; i++) {
      const mx = (xs[i] + xs[i + 1]) / 2, my = (ys[i] + ys[i + 1]) / 2;
      d += ` Q ${xs[i]},${ys[i]} ${mx},${my}`;
    }
    d += ` T ${xs[4]},${ys[4]}`;
    const area = `${d} L ${xs[4]},${H} L ${xs[0]},${H} Z`;
    svg.innerHTML = `
      <defs><linearGradient id="eqg" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#A370FF" stop-opacity="0.38"/><stop offset="100%" stop-color="#A370FF" stop-opacity="0.02"/></linearGradient></defs>
      <line x1="12" y1="${midY}" x2="${W - 12}" y2="${midY}" stroke="rgba(255,255,255,0.12)" stroke-dasharray="4 4"/>
      <text x="14" y="16" fill="#64718c" font-size="9.5" font-family="Consolas">+12 dB</text>
      <text x="14" y="${midY - 4}" fill="#64718c" font-size="9.5" font-family="Consolas">0 dB</text>
      <text x="14" y="${H - 8}" fill="#64718c" font-size="9.5" font-family="Consolas">-12 dB</text>
      <path d="${area}" fill="url(#eqg)"/>
      <path d="${d}" fill="none" stroke="#A370FF" stroke-width="2.6" stroke-linecap="round"/>
      ${xs.map((x, i) => `<circle cx="${x}" cy="${ys[i]}" r="4.5" fill="#fff" stroke="#A370FF" stroke-width="2.2"/>`).join("")}
    `;
  }

  function bindLogitech() {
    document.querySelectorAll("[data-lkind]").forEach((b) => {
      b.onclick = () => {
        logiSelection = { kind: b.dataset.lkind, index: Number(b.dataset.lidx || 0) };
        syncDraftsFromState();
        render();
      };
    });
    const retry = document.getElementById("logi-retry");
    if (retry) { retry.onclick = () => runAction("reconnect_all", {}, "Périphériques Logitech rescannés"); return; }

    const dIdx = logiSelection.index;

    if (logiSelection.kind === "mouse" && getSelectedLogiMouse()) {
      document.querySelectorAll("[data-mvar]").forEach((b) => {
        b.onclick = () => { selectedMouseVariant = Number(b.dataset.mvar); render(); };
      });
      document.querySelectorAll("[data-mbtn]").forEach((b) => {
        b.onclick = () => { selectedMouseBtnIdx = Number(b.dataset.mbtn); render(); };
      });
      const sl = document.getElementById("logi-dpi-slider");
      const sly = document.getElementById("logi-dpiy-slider");
      if (sl) {
        sl.oninput = () => { document.getElementById("logi-dpi-lbl").textContent = `${sl.value} DPI`; };
        if (sly) sly.oninput = () => { document.getElementById("logi-dpiy-lbl").textContent = `${sly.value} DPI`; };
        document.getElementById("logi-dpi-apply").onclick = () =>
          runAction("logi_set_dpi", { device_index: dIdx, x: Number(sl.value), y: sly ? Number(sly.value) : Number(sl.value) }, `DPI appliqué : ${sl.value}`);
      }
      document.querySelectorAll("[data-logi-dpi]").forEach((b) => {
        b.onclick = () => runAction("logi_set_dpi", { device_index: dIdx, x: Number(b.dataset.logiDpi), y: Number(b.dataset.logiDpi) }, `DPI appliqué : ${b.dataset.logiDpi}`);
      });
      document.querySelectorAll("[data-logi-lod]").forEach((b) => {
        b.onclick = () => {
          const cur = getSelectedLogiMouse().dpi;
          runAction("logi_set_dpi", { device_index: dIdx, x: cur.dpi_x, y: cur.dpi_y, lod: b.dataset.logiLod }, `LOD réglé sur ${b.dataset.logiLod}`);
        };
      });
      document.querySelectorAll("[data-logi-hz]").forEach((b) => {
        b.onclick = () => runAction("logi_set_pollrate", { device_index: dIdx, hz: Number(b.dataset.logiHz) }, `Polling Rate souris : ${b.dataset.logiHz} Hz`);
      });
      const asnap = document.getElementById("logi-asnap");
      if (asnap) {
        asnap.onchange = (e) => runAction("logi_set_angle_snapping", { device_index: dIdx, enabled: e.target.checked }, `Angle Snapping : ${e.target.checked ? "Actif" : "Inactif"}`);
      }
      document.querySelectorAll("[data-logi-mode]").forEach((b) => {
        b.onclick = () => runAction("logi_set_mode", { device_index: dIdx, mode: b.dataset.logiMode }, `Mode souris : ${b.textContent}`);
      });
      document.querySelectorAll("[data-logi-prof]").forEach((b) => {
        b.onclick = () => {
          selectedMouseProfileIdx = Number(b.dataset.logiProf);
          syncDraftsFromState();
          render();
        };
      });
      const actBtn = document.getElementById("logi-activate-prof");
      if (actBtn) {
        actBtn.onclick = () => runAction("logi_set_active_profile", { device_index: dIdx, slot: draftProfile.slot_index }, `Profil embarqué #${draftProfile.slot_index} activé !`);
      }
      document.querySelectorAll("[name='def-dpi']").forEach((r) => {
        r.onchange = () => { draftProfile.default_dpi_idx = Number(r.value); };
      });
      document.querySelectorAll("[data-stage-dpi]").forEach((inp) => {
        inp.onchange = () => { draftProfile.dpi_stages[Number(inp.dataset.stageDpi)] = Number(inp.value); };
      });
      document.querySelectorAll("[data-test-stage]").forEach((b) => {
        b.onclick = () => runAction("logi_set_dpi_stage", { device_index: dIdx, stage: Number(b.dataset.testStage) }, `Palier DPI #${Number(b.dataset.testStage) + 1} activé`);
      });
      document.querySelectorAll("[data-btn-remap]").forEach((sel) => {
        sel.onchange = () => {
          const bIdx = Number(sel.dataset.btnRemap);
          const lbl = sel.options[sel.selectedIndex].textContent;
          draftProfile.buttons[bIdx] = { hex: sel.value, label: lbl };
          selectedMouseBtnIdx = bIdx;
          render();
        };
      });
      document.querySelectorAll("[data-prof-hz]").forEach((b) => {
        b.onclick = () => { draftProfile.report_rate_hz = Number(b.dataset.profHz); render(); };
      });
      const rel = document.getElementById("logi-reload-flash");
      if (rel) rel.onclick = () => runAction("logi_reload_profiles", { device_index: dIdx }, "Profils flash relus depuis la souris");
      const fl = document.getElementById("logi-flash-btn");
      if (fl) {
        fl.onclick = () => runAction("logi_flash_profile", {
          device_index: dIdx,
          index: selectedMouseProfileIdx,
          report_rate_hz: draftProfile.report_rate_hz,
          default_dpi_idx: draftProfile.default_dpi_idx,
          dpi_stages: draftProfile.dpi_stages,
          buttons: draftProfile.buttons.map((b) => b.hex),
        }, `Profil #${draftProfile.slot_index} flashé dans la souris et vérifié par CRC-16 !`);
      }
      const fm = document.getElementById("logi-flash-macro-btn");
      if (fm) {
        fm.onclick = () => {
          const kind = document.getElementById("logi-macro-preset").value;
          const presets = {
            double_click: [
              { kind: "mouse_down", value: 1 }, { kind: "delay", value: 15 }, { kind: "mouse_up", value: 1 },
              { kind: "delay", value: 25 },
              { kind: "mouse_down", value: 1 }, { kind: "delay", value: 15 }, { kind: "mouse_up", value: 1 },
            ],
            rapid_e: [
              { kind: "key_down", value: 0x08 }, { kind: "delay", value: 20 }, { kind: "key_up", value: 0x08 },
              { kind: "delay", value: 30 },
              { kind: "key_down", value: 0x08 }, { kind: "delay", value: 20 }, { kind: "key_up", value: 0x08 },
            ],
            jump_crouch: [
              { kind: "key_down", value: 0x2C }, { kind: "delay", value: 25 }, { kind: "key_down", value: 0xE0 },
              { kind: "delay", value: 60 }, { kind: "key_up", value: 0xE0 }, { kind: "key_up", value: 0x2C },
            ],
          };
          runAction("logi_flash_macro", {
            device_index: dIdx,
            profile_index: selectedMouseProfileIdx,
            button_index: selectedMouseBtnIdx,
            steps: presets[kind] || presets.double_click,
          }, `Macro matérielle flashée dans la souris et assignée à M${selectedMouseBtnIdx + 1} !`);
        };
      }
    } else if (logiSelection.kind === "keyboard" && getSelectedLogiKeyboard()) {
      document.getElementById("lkb-apply-rgb").onclick = () => runAction("logi_kb_set_rgb", {
        device_index: dIdx,
        mode: document.getElementById("lkb-mode").value,
        color: document.getElementById("lkb-col").value,
      }, "Éclairage clavier Logitech appliqué !");
      const br = document.getElementById("lkb-br");
      if (br) {
        br.oninput = () => { document.getElementById("lkb-br-lbl").textContent = `${br.value}%`; };
        br.onchange = () => runAction("logi_kb_set_brightness", { device_index: dIdx, percent: Number(br.value) }, `Luminosité clavier : ${br.value}%`);
      }
    } else if (logiSelection.kind === "headset" && getSelectedLogiHeadset()) {
      document.querySelectorAll("[data-hvar]").forEach((b) => {
        b.onclick = () => { selectedHeadsetVariant = Number(b.dataset.hvar); render(); };
      });
      renderEqSvg();
      const side = document.getElementById("hs-sidetone");
      side.oninput = () => { document.getElementById("hs-side-pill").textContent = `${side.value}%`; };
      side.onchange = () => runAction("logi_set_sidetone", { device_index: dIdx, percent: Number(side.value) }, `Sidetone PRO X réglé sur ${side.value}%`);
      document.getElementById("hs-side-off").onclick = () => runAction("logi_set_sidetone", { device_index: dIdx, percent: 0 }, "Sidetone PRO X coupé (0%)");
      document.querySelectorAll("[data-eq-preset]").forEach((b) => {
        b.onclick = () => {
          const pr = getSelectedLogiHeadset().presets[Number(b.dataset.eqPreset)];
          eqDraft = [...pr.gains];
          runAction("logi_set_eq", { device_index: dIdx, gains: eqDraft }, `Preset Pro '${pr.name}' appliqué au casque PRO X !`);
        };
      });
      document.querySelectorAll("[data-eq-band]").forEach((r) => {
        r.oninput = () => {
          const i = Number(r.dataset.eqBand);
          eqDraft[i] = Number(r.value);
          document.getElementById(`eq-db-${i}`).textContent = `${eqDraft[i] > 0 ? "+" : ""}${eqDraft[i]} dB`;
          renderEqSvg();
        };
      });
      document.getElementById("hs-apply-eq").onclick = () =>
        runAction("logi_set_eq", { device_index: dIdx, gains: eqDraft }, `Égaliseur matériel appliqué : [${eqDraft.join(", ")}] dB`);
      document.getElementById("hs-reset-eq").onclick = () => {
        eqDraft = [0, 0, 0, 0, 0];
        runAction("logi_set_eq", { device_index: dIdx, gains: eqDraft }, "Égaliseur remis à plat (0 dB)");
      };
      // Native WASAPI Audio Hub bindings
      const outVol = document.getElementById("wa-out-vol");
      if (outVol) {
        outVol.oninput = () => { document.getElementById("wa-out-lbl").textContent = `${outVol.value}%`; };
        outVol.onchange = () => runAction("audio_set_volume", { is_mic: false, percent: Number(outVol.value) }, `Volume Windows : ${outVol.value}%`, false);
      }
      const outMute = document.getElementById("wa-out-mute");
      if (outMute) outMute.onclick = () => runAction("audio_set_mute", { is_mic: false, muted: !(S.telemetry?.audio?.output_muted) }, "Mute sortie audio basculé");
      const micVol = document.getElementById("wa-mic-vol");
      if (micVol) {
        micVol.oninput = () => { document.getElementById("wa-mic-lbl").textContent = `${micVol.value}%`; };
        micVol.onchange = () => runAction("audio_set_volume", { is_mic: true, percent: Number(micVol.value) }, `Volume Micro : ${micVol.value}%`, false);
      }
      const micMute = document.getElementById("wa-mic-mute");
      if (micMute) micMute.onclick = () => runAction("audio_set_mute", { is_mic: true, muted: !(S.telemetry?.audio?.mic_muted) }, "Mute micro basculé");
    }
  }

  // ==================== 6. ESPORTS, SOCD / SNAP TAP, TOUCHES & MACROS ====================
  function viewMacros() {
    const prof = S.config.profile;
    const bindings = Object.entries(prof.bindings || {});
    const socd = prof.socd || { enabled: false, pairs: [{ key_a: "Q", key_b: "D" }, { key_a: "Z", key_b: "S" }], blocked_apps: ["cs2.exe"] };
    const sp = S.system_prefs || {};
    const linked = prof.linked_apps || [];
    const pairLabel = (socd.pairs || []).map((p) => `${p.key_a}+${p.key_b}`).join("  •  ") || "Q+D • Z+S";
    const presetGuess = pairLabel.includes("A+D")
      ? (socd.pairs.length === 1 ? "ad_only" : "qwerty")
      : (socd.pairs.length === 1 ? "qd_only" : "azerty");

    return `
      <div class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar purple"></div>
          <div>
            <div class="page-title">Esports Boosters, SOCD / Snap Tap &amp; Macros</div>
            <div class="page-sub">SOCD Last-Input Priority • Protection Anti-Kick CS2 • Anti-Chatter &amp; Anti-Double-Clic • Enregistreur Live • Auto-Switch Jeux</div>
          </div>
        </div>
        <div class="header-actions">
          <label class="switch">
            <input type="checkbox" id="mac-engine-on" ${S.input_running ? "checked" : ""} />
            <span class="switch-track"></span>
            <span>Moteur Basse Latence</span>
          </label>
          <label class="switch">
            <input type="checkbox" id="mac-winlock" ${prof.win_lock ? "checked" : ""} />
            <span class="switch-track"></span>
            <span>WinLock</span>
          </label>
        </div>
      </div>

      <!-- ESPORTS OPTIMIZATION SUITE -->
      <div class="grid-2 mb-16" style="margin-bottom:16px">
        <div class="card accent-purple">
          <div class="card-head">
            <div>
              <div class="card-title">SOCD / Snap Tap (Last-Input Priority)</div>
              <div class="muted">Counter-strafe instantané sur n'importe quel clavier (${esc(pairLabel)})</div>
            </div>
            <span class="pill ${socd.enabled ? (sp.socd_suspended ? "amber" : "emerald") : ""}">
              <span class="dot"></span>${socd.enabled ? (sp.socd_suspended ? "SUSPENDU (Jeu protégé)" : "ACTIF") : "OFF"}
            </span>
          </div>
          <div class="row-between">
            <label class="switch">
              <input type="checkbox" id="es-socd-en" ${socd.enabled ? "checked" : ""} />
              <span class="switch-track"></span>
              <span>Activer SOCD / Snap Tap</span>
            </label>
            <select id="es-socd-preset" class="og-select" style="width:210px">
              <option value="azerty" ${presetGuess === "azerty" ? "selected" : ""}>AZERTY (Q+D &amp; Z+S)</option>
              <option value="qd_only" ${presetGuess === "qd_only" ? "selected" : ""}>AZERTY Strafe seul (Q+D)</option>
              <option value="qwerty" ${presetGuess === "qwerty" ? "selected" : ""}>QWERTY (A+D &amp; W+S)</option>
              <option value="ad_only" ${presetGuess === "ad_only" ? "selected" : ""}>QWERTY Strafe seul (A+D)</option>
            </select>
          </div>
          <div class="mt-12" style="padding-top:10px;border-top:1px solid var(--border)">
            <div class="row-between" style="margin-bottom:6px">
              <span class="muted">Protection Anti-Kick (auto-suspendu sur ces .exe) :</span>
              <span class="pill emerald" style="font-size:10px">Valve CS2 protégé</span>
            </div>
            <div class="row">
              <input id="es-socd-blocked" class="og-input mono" style="flex:1" value="${esc((socd.blocked_apps || []).join(", "))}" placeholder="cs2.exe" />
              <button class="btn btn-sm btn-secondary" id="es-socd-save-bl">Appliquer</button>
            </div>
          </div>
        </div>

        <div class="card accent-cyan">
          <div class="card-head">
            <div>
              <div class="card-title">Boucliers Anti-Rebond &amp; Testeur Esports Live</div>
              <div class="muted">Filtre logiciels 100 % safe (ne touche jamais aux deltas souris Raw Input)</div>
            </div>
            <span class="pill cyan">Bloqués : ${sp.chatter_blocked || 0} kb • ${sp.mouse_dblclick_blocked || 0} clic</span>
          </div>
          <div class="row-between">
            <span class="muted" style="width:175px">Anti-Chatter Clavier</span>
            <input type="range" id="es-chatter-ms" class="og-range" style="flex:1" min="0" max="60" step="1" value="${prof.key_chatter_ms || 0}" />
            <span class="mono" id="es-chatter-lbl" style="width:52px;text-align:right">${prof.key_chatter_ms || 0} ms</span>
          </div>
          <div class="row-between mt-12">
            <span class="muted" style="width:175px">Anti-Double-Clic Souris</span>
            <input type="range" id="es-mouse-deb-ms" class="og-range" style="flex:1" min="0" max="60" step="1" value="${prof.mouse_debounce_ms || 0}" />
            <span class="mono" id="es-mouse-deb-lbl" style="width:52px;text-align:right">${prof.mouse_debounce_ms || 0} ms</span>
          </div>
          <div class="row-between mt-12" style="padding-top:10px;border-top:1px solid var(--border)">
            <button class="btn btn-sm btn-secondary" id="es-click-pad" style="flex:1;padding:10px">Clique ici rapidement pour tester ton intervalle de clic (ms) &amp; double-clic</button>
            <span class="mono pill" id="es-click-res" style="min-width:115px;justify-content:center">Prêt</span>
          </div>
        </div>
      </div>

      ${renderKeyboardHtml("macro", getSelectedCorsair())}

      <div class="grid-2 mt-16">
        <div class="card">
          <div class="card-head">
            <div class="card-title">Configurer la touche sélectionnée</div>
            <span class="pill purple">${esc(selectedKeyId)}</span>
          </div>
          <div class="row">
            <span class="muted">Remapper vers :</span>
            <select id="mac-remap-target" class="og-select" style="flex:1">
              ${S.bindable_keys.map((k) => `<option value="${k}" ${k === "F13" ? "selected" : ""}>${k}</option>`).join("")}
            </select>
            <button class="btn btn-primary btn-sm" id="mac-apply-remap">Appliquer</button>
          </div>
          <div class="row mt-12">
            <button class="btn btn-secondary btn-sm" id="mac-disable-key">Désactiver la touche</button>
            <button class="btn btn-secondary btn-sm" id="mac-reset-key">Réinitialiser</button>
          </div>

          <div class="mt-16" style="padding-top:12px;border-top:1px solid var(--border)">
            <div class="muted" style="margin-bottom:6px;font-weight:600">Macro Texte (Unicode) :</div>
            <div class="row">
              <input id="mac-text-val" class="og-input" style="flex:1" placeholder="Ex: gg wp!" />
              <button class="btn btn-secondary btn-sm" id="mac-apply-text">Assigner</button>
            </div>
          </div>

          <div class="mt-16" style="padding-top:12px;border-top:1px solid var(--border)">
            <div class="row-between" style="margin-bottom:8px">
              <span class="muted" style="font-weight:600">Macro Multi-Étapes &amp; Enregistreur Live :</span>
              ${sp.macro_recording
                ? `<button class="btn btn-sm btn-rog" id="mac-rec-stop">■ Arrêter &amp; Capturer</button>`
                : `<button class="btn btn-sm btn-primary" id="mac-rec-start">● Enregistrer en direct</button>`}
            </div>
            <div class="row">
              <input id="mac-name" class="og-input" style="width:120px" value="Macro 1" />
              <select id="mac-step-key" class="og-select" style="width:105px">
                ${S.bindable_keys.map((k) => `<option value="${k}">${k}</option>`).join("")}
              </select>
              <input id="mac-step-ms" type="number" class="og-input" style="width:68px" value="15" />
              <button class="btn btn-secondary btn-sm" id="mac-add-tap">+ Tap</button>
              <button class="btn btn-secondary btn-sm" id="mac-clear-steps">Vider</button>
            </div>
            <div class="mono muted mt-12" style="max-height:95px;overflow:auto;background:#0b0e16;padding:8px;border-radius:8px">
              ${sp.macro_recording
                ? `<span style="color:var(--rog);font-weight:700">● ENREGISTREMENT EN COURS… Tape ta combinaison puis clique sur « Arrêter &amp; Capturer ».</span>`
                : (macroSteps.map((st, i) => `${i + 1}. ${st.down ? "DOWN" : "UP  "} ${st.key} (${st.delay_ms} ms)`).join("<br/>") || "Aucune étape")}
            </div>
            <button class="btn btn-primary btn-sm mt-12" id="mac-apply-steps">Assigner la macro à ${esc(selectedKeyId)}</button>
          </div>
        </div>

        <div class="stack">
          <div class="card">
            <div class="card-head">
              <div class="card-title">Assignations actives sur "${esc(prof.name)}"</div>
              <span class="pill">${bindings.length} règle(s)</span>
            </div>
            ${bindings.length === 0 ? `<div class="muted">Aucune assignation sur ce profil. Clique sur une touche ci-dessus pour commencer.</div>` : `
              <table class="meta-table">
                <thead><tr><th>Touche</th><th>Action</th><th></th></tr></thead>
                <tbody>
                  ${bindings.map(([k, act]) => {
                    let desc = "";
                    if (act.type === "Remap") desc = `→ ${act.target}`;
                    else if (act.type === "Disable") desc = "Désactivée";
                    else if (act.type === "Text") desc = `Texte "${act.text}"`;
                    else if (act.type === "Macro") desc = `Macro '${act.name}' (${act.steps.length} étapes)`;
                    return `<tr><td class="mono" style="color:var(--cyan);font-weight:700">${esc(k)}</td><td>${esc(desc)}</td><td style="text-align:right"><button class="btn btn-sm btn-secondary" data-rm-bind="${esc(k)}">Suppr</button></td></tr>`;
                  }).join("")}
                </tbody>
              </table>
            `}
          </div>

          <div class="card accent-emerald">
            <div class="card-head">
              <div>
                <div class="card-title">Auto-Switch Jeux pour "${esc(prof.name)}"</div>
                <div class="muted">Active automatiquement ce profil dès que l'un de ces .exe est au premier plan</div>
              </div>
              <span class="pill emerald">${linked.length} app(s)</span>
            </div>
            <div class="row">
              <input id="prof-linked-input" class="og-input mono" style="flex:1" value="${esc(linked.join(", "))}" placeholder="Ex: valorant-win64-shipping.exe, r5apex.exe" />
              <button class="btn btn-sm btn-primary" id="prof-linked-save">Enregistrer</button>
            </div>
            ${(sp.visible_apps || []).length ? `
              <div class="muted mt-12" style="font-size:11px;margin-bottom:6px">Apps ouvertes (clic pour lier à ce profil) :</div>
              <div class="row" style="gap:6px;flex-wrap:wrap">
                ${sp.visible_apps.slice(0, 10).map((app) => `<button class="btn btn-sm btn-secondary mono" data-link-app="${esc(app)}">+ ${esc(app)}</button>`).join("")}
              </div>
            ` : ""}
          </div>
        </div>
      </div>
    `;
  }

  function bindMacros() {
    document.querySelectorAll(".kb-key").forEach((k) => {
      k.onclick = () => { selectedKeyId = k.dataset.keyId; render(); };
    });
    document.getElementById("mac-engine-on").onchange = (e) =>
      runAction("input_set_enabled", { enabled: e.target.checked }, e.target.checked ? "Moteur basse latence Windows démarré !" : "Moteur arrêté");
    document.getElementById("mac-winlock").onchange = (e) =>
      runAction("input_set_winlock", { enabled: e.target.checked }, `WinLock logiciel : ${e.target.checked ? "Actif" : "Inactif"}`);

    // SOCD / Snap Tap & Debounce Shields
    document.getElementById("es-socd-en").onchange = (e) =>
      runAction("input_set_esports", { socd_enabled: e.target.checked }, e.target.checked ? "SOCD / Snap Tap activé (protection CS2 active) !" : "SOCD / Snap Tap désactivé");
    document.getElementById("es-socd-preset").onchange = (e) =>
      runAction("input_set_esports", { socd_preset: e.target.value }, `Paires SOCD mises à jour (${e.target.options[e.target.selectedIndex].text})`);
    document.getElementById("es-socd-save-bl").onclick = () => {
      const list = document.getElementById("es-socd-blocked").value.split(",").map((s) => s.trim()).filter(Boolean);
      runAction("input_set_esports", { blocked_apps: list }, `Protection anti-kick SOCD mise à jour (${list.join(", ") || "aucune"})`);
    };
    const chEl = document.getElementById("es-chatter-ms");
    chEl.oninput = () => { document.getElementById("es-chatter-lbl").textContent = `${chEl.value} ms`; };
    chEl.onchange = () => runAction("input_set_esports", { key_chatter_ms: Number(chEl.value) }, `Anti-Chatter clavier : ${chEl.value} ms`);
    const mdEl = document.getElementById("es-mouse-deb-ms");
    mdEl.oninput = () => { document.getElementById("es-mouse-deb-lbl").textContent = `${mdEl.value} ms`; };
    mdEl.onchange = () => runAction("input_set_esports", { mouse_debounce_ms: Number(mdEl.value) }, `Anti-Double-Clic souris : ${mdEl.value} ms`);

    // Live Double-Click & Click Interval Tester
    let lastClickTs = 0;
    const pad = document.getElementById("es-click-pad");
    const res = document.getElementById("es-click-res");
    pad.onmousedown = () => {
      const now = performance.now();
      if (lastClickTs > 0) {
        const dt = Math.round(now - lastClickTs);
        res.textContent = `${dt} ms ${dt < 45 ? "⚠ Rapide!" : "OK"}`;
        res.className = `mono pill ${dt < 45 ? "rog" : "emerald"}`;
      } else {
        res.textContent = "1er clic";
      }
      lastClickTs = now;
    };

    // Live Macro Recorder
    const recStart = document.getElementById("mac-rec-start");
    if (recStart) recStart.onclick = () => runAction("input_macro_record_start", {}, "Enregistrement live démarré — tape ta combinaison !");
    const recStop = document.getElementById("mac-rec-stop");
    if (recStop) recStop.onclick = async () => {
      try {
        const resp = await rpc("input_macro_record_stop", {});
        if (resp.recorded_steps?.length) {
          macroSteps = resp.recorded_steps;
          toast(`${macroSteps.length} étapes capturées en direct !`);
        } else {
          toast("Aucune touche capturée", true);
        }
        if (resp.state) S = resp.state;
        render();
      } catch (e) { toast(String(e), true); }
    };

    // Profile Auto-Switch Linked Apps
    document.getElementById("prof-linked-save").onclick = () => {
      const apps = document.getElementById("prof-linked-input").value.split(",").map((s) => s.trim()).filter(Boolean);
      runAction("profile_set_linked_apps", { apps }, `Auto-Switch lié à ${apps.length} app(s)`);
    };
    document.querySelectorAll("[data-link-app]").forEach((b) => {
      b.onclick = () => {
        const inp = document.getElementById("prof-linked-input");
        const cur = inp.value.split(",").map((s) => s.trim()).filter(Boolean);
        if (!cur.includes(b.dataset.linkApp)) cur.push(b.dataset.linkApp);
        runAction("profile_set_linked_apps", { apps: cur }, `${b.dataset.linkApp} lié au profil "${S.config.profile.name}" !`);
      };
    });

    document.getElementById("mac-apply-remap").onclick = () =>
      runAction("input_set_binding", { key: selectedKeyId, kind: "remap", target: document.getElementById("mac-remap-target").value }, `${selectedKeyId} remappée`);
    document.getElementById("mac-disable-key").onclick = () =>
      runAction("input_set_binding", { key: selectedKeyId, kind: "disable" }, `${selectedKeyId} désactivée`);
    document.getElementById("mac-reset-key").onclick = () =>
      runAction("input_remove_binding", { key: selectedKeyId }, `${selectedKeyId} réinitialisée`);
    document.getElementById("mac-apply-text").onclick = () =>
      runAction("input_set_binding", { key: selectedKeyId, kind: "text", text: document.getElementById("mac-text-val").value, delay_ms: 15 }, `Macro texte assignée à ${selectedKeyId}`);
    document.getElementById("mac-add-tap").onclick = () => {
      const k = document.getElementById("mac-step-key").value;
      const ms = Number(document.getElementById("mac-step-ms").value) || 15;
      macroSteps.push({ key: k, down: true, delay_ms: ms }, { key: k, down: false, delay_ms: ms });
      render();
    };
    document.getElementById("mac-clear-steps").onclick = () => { macroSteps = []; render(); };
    document.getElementById("mac-apply-steps").onclick = () =>
      runAction("input_set_binding", { key: selectedKeyId, kind: "macro", name: document.getElementById("mac-name").value, steps: macroSteps }, `Macro assignée à ${selectedKeyId}`);
    document.querySelectorAll("[data-rm-bind]").forEach((b) => {
      b.onclick = () => runAction("input_remove_binding", { key: b.dataset.rmBind }, `Règle ${b.dataset.rmBind} supprimée`);
    });
  }

  // ==================== 7. DEVICES (HID BUS & FIRMWARE MONITOR) ====================
  function viewDevices() {
    const groups = ["ASUS", "Corsair", "Logitech", "Alienware"];
    const fws = S.firmware_inventory || [];
    return `
      <div class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar emerald"></div>
          <div>
            <div class="page-title">Mon Setup — Inventaire Firmware &amp; Bus HID</div>
            <div class="page-sub">${S.devices.length} interfaces HID détectées • Hot-Plug automatique toutes les 3.5s</div>
          </div>
        </div>
        <div class="header-actions">
          <button class="btn btn-secondary" id="dev-scan-known">Périphériques Gaming</button>
          <button class="btn btn-secondary" id="dev-scan-all">Tout le bus HID</button>
        </div>
      </div>

      ${(() => {
        const st = S.selftest;
        return `
          <div class="card accent-purple" style="margin-bottom:16px">
            <div class="card-head">
              <div>
                <div class="card-title">Auto-Test Matériel Précis (Vérification Read-Back &amp; Diagnostic)</div>
                <div class="muted">Teste tous les contrôleurs (Corsair, Logitech, Aura, WASAPI, PawnIO CPU/SuperIO/SMBus RAM) et génère <span class="mono">%TEMP%\\opengear_selftest.txt</span></div>
              </div>
              <div class="row">
                ${st ? `<span class="pill ${st.failed ? "rog" : "emerald"}">${st.passed} PASS • ${st.warned} WARN • ${st.failed} FAIL</span>` : ""}
                <button class="btn btn-primary btn-sm" id="dev-run-selftest">Lancer Auto-Test Complet</button>
              </div>
            </div>
            ${st ? `
              <table class="meta-table">
                <thead><tr><th>Statut</th><th>Sous-système</th><th>Détail vérifié</th></tr></thead>
                <tbody>
                  ${st.checks.map((c) => `
                    <tr>
                      <td><span class="pill ${c.status === "PASS" ? "emerald" : (c.status === "FAIL" ? "rog" : "amber")}">${esc(c.status)}</span></td>
                      <td style="font-weight:600">${esc(c.name)}</td>
                      <td class="mono" style="font-size:11.5px">${esc(c.detail)}</td>
                    </tr>
                  `).join("")}
                </tbody>
              </table>
              <div class="muted mt-12 mono" style="font-size:11px">Rapport exporté : ${esc(st.report_path)}</div>
            ` : `<div class="muted">Clique sur <strong>Lancer Auto-Test Complet</strong> (en mode Admin pour inclure PawnIO SuperIO + SMBus RAM) pour obtenir un diagnostic exact registre par registre.</div>`}
          </div>
        `;
      })()}

      ${fws.length ? `
        <div class="card accent-cyan" style="margin-bottom:16px">
          <div class="card-head">
            <div>
              <div class="card-title">Moniteur Firmware (Lecture 100 % Safe — Zéro risque de brick)</div>
              <div class="muted">Versions lues en direct depuis les MCU de tes périphériques + portails officiels de mise à jour</div>
            </div>
            <span class="pill emerald">${fws.length} composant(s)</span>
          </div>
          <table class="meta-table">
            <thead><tr><th>Périphérique</th><th>Marque</th><th>Firmware Actuel</th><th>Statut</th></tr></thead>
            <tbody>
              ${fws.map((f) => `
                <tr>
                  <td style="font-weight:600">${esc(f.device)}</td>
                  <td>${esc(f.vendor)}</td>
                  <td class="mono" style="color:var(--cyan)">${esc(f.version)}</td>
                  <td><span class="pill emerald">${esc(f.status)}</span></td>
                </tr>
              `).join("")}
            </tbody>
          </table>
        </div>
      ` : ""}

      <div class="stack">
        ${groups.map((vendor) => {
          const list = S.devices.filter((d) => d.vendor === vendor);
          if (!list.length) return "";
          return `
            <div class="card">
              <div class="card-head">
                <span class="pill ${vendor === "ASUS" ? "rog" : vendor === "Corsair" ? "cyan" : vendor === "Logitech" ? "amber" : "purple"}"><span class="dot"></span>${vendor}</span>
                <span class="muted">${list.length} interfaces</span>
              </div>
              <table class="meta-table">
                <thead><tr><th>VID:PID</th><th>IF</th><th>UsagePage</th><th>Usage</th><th>Produit</th><th>Statut OpenGear</th></tr></thead>
                <tbody>
                  ${list.map((d) => `
                    <tr>
                      <td class="mono">${d.vid_pid}</td>
                      <td class="mono">${d.interface}</td>
                      <td class="mono">${d.usage_page}</td>
                      <td class="mono">${d.usage}</td>
                      <td>${esc(d.product)}</td>
                      <td style="color:${d.active ? "var(--emerald)" : "var(--text-2)"};font-weight:${d.active ? 650 : 400}">${esc(d.status)}</td>
                    </tr>
                  `).join("")}
                </tbody>
              </table>
            </div>
          `;
        }).join("")}
      </div>
    `;
  }

  function bindDevices() {
    document.getElementById("dev-scan-known").onclick = () => runAction("scan_hid", { all: false }, "Bus HID gaming rescanné");
    document.getElementById("dev-scan-all").onclick = () => runAction("scan_hid", { all: true }, "Bus HID complet rescanné");
    const stBtn = document.getElementById("dev-run-selftest");
    if (stBtn) stBtn.onclick = () => runAction("run_selftest", {}, "Auto-Test Matériel terminé — rapport écrit dans %TEMP%\\opengear_selftest.txt !");
  }

  // ==================== SIDEBAR NAVIGATION & BOOT ====================
  function highlightNav() {
    document.querySelectorAll(".nav-item").forEach((b) => {
      b.classList.toggle("active", b.dataset.tab === activeTab);
    });
  }

  document.querySelectorAll(".nav-item").forEach((b) => {
    b.onclick = () => {
      activeTab = b.dataset.tab;
      highlightNav();
      render();
    };
  });
  document.getElementById("profile-select").onchange = (e) =>
    runAction("switch_profile", { name: e.target.value }, `Profil '${e.target.value}' chargé`);
  document.getElementById("btn-save-cfg").onclick = () =>
    runAction("save_config", {}, "Configuration enregistrée sur disque !", false);
  document.getElementById("btn-rescan").onclick = () =>
    runAction("reconnect_all", {}, "Tous les périphériques ont été reconnectés !");
  document.getElementById("btn-export-prof").onclick = async () => {
    try {
      const res = await rpc("profile_export_json", {});
      const blob = new Blob([res.json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `opengear-profile-${(res.name || "default").toLowerCase().replace(/[^a-z0-9_-]/g, "_")}.json`;
      a.click();
      URL.revokeObjectURL(url);
      toast(`Profil '${res.name}' exporté en JSON !`);
    } catch (e) { toast(String(e), true); }
  };
  document.getElementById("btn-import-prof").onclick = () => {
    const inp = document.createElement("input");
    inp.type = "file";
    inp.accept = ".json";
    inp.onchange = async () => {
      const f = inp.files?.[0];
      if (!f) return;
      try {
        const text = await f.text();
        await runAction("profile_import_json", { json: text }, `Profil importé depuis ${f.name} !`);
      } catch (e) { toast(String(e), true); }
    };
    inp.click();
  };
  document.getElementById("btn-add-profile").onclick = () => {
    const inp = document.getElementById("new-profile-input");
    const name = inp.value.trim();
    if (!name) return;
    inp.value = "";
    runAction("create_profile", { name }, `Profil '${name}' créé et sauvegardé`);
  };
  document.getElementById("chk-azerty").onchange = (e) =>
    runAction("set_azerty", { enabled: e.target.checked });

  // Initial load
  refreshState().then(() => {
    setStatus(`Prêt — ${totalActiveDevices()} contrôleurs Plug & Play actifs en natif`);
  }).catch((e) => {
    document.getElementById("main-content").innerHTML = `<div class="card accent-rog">Erreur d'initialisation : ${esc(e.message)}</div>`;
  });
})();
