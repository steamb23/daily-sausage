import * as api from "./backend";
import type { Config, Gauge, Row, Section, Snapshot } from "./types";

type Child = Node | string | null | undefined | false;

function h(tag: string, attrs: Record<string, string | boolean> = {}, ...children: Child[]): HTMLElement {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === false) continue;
    if (v === true) el.setAttribute(k, "");
    else el.setAttribute(k, v);
  }
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c);
  }
  return el;
}

const $main = document.getElementById("view-main")!;
const $settings = document.getElementById("view-settings")!;
const $footer = document.getElementById("footer-text")!;
const $btnSettings = document.getElementById("btn-settings")!;
const $btnRefresh = document.getElementById("btn-refresh")!;

let snapshot: Snapshot | null = null;
let config: Config | null = null;
let showingSettings = false;

// ---------- main view ----------

function gauge(g: Gauge, kind: "sausage" | "mini"): HTMLElement {
  const fill = h("div", { class: "fill" });
  fill.style.width = `${Math.round(Math.max(0, Math.min(1, g.ratio)) * 1000) / 10}%`;
  const el = h("div", { class: `gauge ${kind} level-${g.level}`, role: "presentation" }, fill);
  if (g.marker !== null) {
    const m = h("div", { class: "marker", title: "시간 기준 이상선" });
    m.style.left = `${Math.round(g.marker * 1000) / 10}%`;
    el.append(m);
  }
  return el;
}

function section(s: Section, cls: string): HTMLElement {
  return h(
    "section",
    { class: `card ${cls}` },
    h("div", { class: "card-head" }, h("span", { class: "title" }, s.title), h("span", { class: "right" }, s.right)),
    h(
      "div",
      { class: "big-row" },
      h("span", { class: `big level-${s.gauge.level}` }, s.big),
      s.big_note ? h("span", { class: "big-note" }, s.big_note) : null,
    ),
    gauge(s.gauge, "sausage"),
    ...s.lines.map((l, i) => h("p", { class: i === 0 ? "line" : "line sub" }, l)),
    s.note ? h("p", { class: "note" }, s.note) : null,
  );
}

function row(r: Row): HTMLElement {
  return h(
    "div",
    { class: "row" },
    h("span", { class: "label" }, r.label),
    gauge(r.gauge, "mini"),
    h("span", { class: `pct level-${r.gauge.level}` }, r.big),
    h("span", { class: "right" }, r.right),
  );
}

function renderMain(): void {
  $main.replaceChildren();
  if (!snapshot) {
    $main.append(h("p", { class: "empty" }, "사용량을 불러오는 중이에요…"));
    $footer.textContent = "";
    return;
  }
  const m = snapshot.model;
  document.body.classList.toggle("stale", m.stale && !!m.today);
  if (m.today) $main.append(section(m.today, "today"));
  if (m.week) $main.append(section(m.week, "week"));
  if (m.rows.length) $main.append(h("div", { class: "rows" }, ...m.rows.map(row)));
  if (m.empty) {
    const go = h("button", { class: "btn primary" }, "설정 열기");
    go.addEventListener("click", () => toggleSettings(true));
    $main.append(h("div", { class: "empty" }, h("p", {}, m.empty), go));
  }
  if (snapshot.oauth_enabled && snapshot.oauth.message) {
    $main.append(h("p", { class: "warn-text" }, `사용량 API: ${snapshot.oauth.message}`));
  }
  $footer.textContent = m.footer;
}

// ---------- settings view ----------

function field(label: string, control: HTMLElement, help?: string): HTMLElement {
  return h("div", { class: "field" }, h("label", { class: "field-label" }, label, control), help ? h("p", { class: "help" }, help) : null);
}

function checkbox(checked: boolean, onChange: (v: boolean) => void): HTMLInputElement {
  const el = h("input", { type: "checkbox", checked }) as HTMLInputElement;
  el.addEventListener("change", () => onChange(el.checked));
  return el;
}

function select(options: [string, string][], value: string, onChange: (v: string) => void): HTMLSelectElement {
  const el = h("select", {}, ...options.map(([v, t]) => h("option", { value: v, selected: v === value }, t))) as HTMLSelectElement;
  el.addEventListener("change", () => onChange(el.value));
  return el;
}

async function save(patch: Partial<Config>): Promise<void> {
  if (!config) return;
  try {
    config = await api.setConfig({ ...config, ...patch });
  } catch (e) {
    alertBox(String(e));
  }
  renderSettings();
}

function alertBox(text: string): void {
  const box = document.getElementById("settings-message");
  if (box) {
    box.textContent = text;
    box.hidden = false;
  }
}

function statuslineBlock(): HTMLElement {
  const st = snapshot?.statusline ?? { state: "not_configured" as const };
  const wrap = h("div", { class: "group" }, h("h3", {}, "Claude Code 연동 (공식 데이터)"));
  const desc = {
    not_configured: "아직 설치하지 않았어요. 설치하면 Claude Code가 응답을 받을 때마다 사용량이 기록돼요.",
    installed: "설치되어 있어요. Claude Code를 쓰는 동안 사용량이 자동으로 기록돼요.",
    other: "다른 상태줄이 설정되어 있어요. 설치하면 기존 상태줄은 그대로 보이도록 이어서 실행해요.",
    unreadable: "Claude Code 설정 파일을 읽지 못했어요.",
  }[st.state];
  wrap.append(h("p", { class: "help" }, desc));
  if (st.state === "unreadable") wrap.append(h("p", { class: "warn-text" }, st.error));

  const preview = h("div", { class: "preview", hidden: true });
  const actions = h("div", { class: "btn-row" });
  if (st.state === "installed") {
    const un = h("button", { class: "btn" }, "연동 제거");
    un.addEventListener("click", async () => {
      try {
        alertBox(await api.statuslineUninstall());
      } catch (e) {
        alertBox(String(e));
      }
    });
    actions.append(un);
  } else if (st.state !== "unreadable") {
    const inst = h("button", { class: "btn primary" }, "연동 설치…");
    inst.addEventListener("click", async () => {
      try {
        const p = await api.statuslinePreview();
        const apply = h("button", { class: "btn primary" }, "이대로 적용");
        const cancel = h("button", { class: "btn" }, "취소");
        apply.addEventListener("click", async () => {
          try {
            alertBox(await api.statuslineInstall());
          } catch (e) {
            alertBox(String(e));
          }
          preview.hidden = true;
        });
        cancel.addEventListener("click", () => (preview.hidden = true));
        preview.replaceChildren();
        preview.append(
          h("p", { class: "help" }, `${p.settings_path}의 statusLine을 아래처럼 바꿔요. 적용 전에 원본을 백업해요.`),
          h("div", { class: "diff" }, h("div", {}, h("strong", {}, "지금"), h("pre", {}, p.before)), h("div", {}, h("strong", {}, "바꾼 뒤"), h("pre", {}, p.after))),
          h("div", { class: "btn-row" }, apply, cancel),
        );
        if (p.chains) preview.insertBefore(h("p", { class: "help" }, `기존 명령 "${p.chains}"은 계속 실행돼서 상태줄 모양은 그대로예요.`), preview.lastChild);
        preview.hidden = false;
      } catch (e) {
        alertBox(String(e));
      }
    });
    actions.append(inst);
  }
  wrap.append(actions, preview);
  if (config) {
    wrap.append(
      field(
        "기존 상태줄이 없으면 요약 한 줄 표시",
        checkbox(config.statusline_summary, (v) => save({ statusline_summary: v })),
        "Claude Code 하단에 오늘 예산 사용률과 남은 몫, 주간 사용률을 한 줄로 보여줘요.",
      ),
    );
  }
  return wrap;
}

function oauthBlock(c: Config): HTMLElement {
  const wrap = h("div", { class: "group" }, h("h3", {}, "사용량 API 조회 (비공식, 개인용)"));
  wrap.append(
    h(
      "p",
      { class: "help" },
      "Claude Code에 저장된 내 로그인으로 api.anthropic.com의 사용량 정보를 직접 조회해요. Claude Code를 쓰지 않을 때도, 데스크탑 앱이나 웹에서 쓴 양까지 바로 반영돼요.",
    ),
  );
  const toggle = checkbox(c.oauth_enabled, (v) => {
    if (v && !c.oauth_acknowledged) {
      toggle.checked = false;
      notice.hidden = false;
      return;
    }
    save({ oauth_enabled: v });
  });
  const ok = h("button", { class: "btn primary" }, "이해했어요, 켤게요");
  const no = h("button", { class: "btn" }, "취소");
  const notice = h(
    "div",
    { class: "notice", hidden: true },
    h("p", {}, "이 기능은 문서화되지 않은 내부 API를 사용해요."),
    h(
      "ul",
      {},
      h("li", {}, "Anthropic이 언제든 바꾸거나 막을 수 있어요."),
      h("li", {}, "로그인 토큰은 이 PC의 메모리에서만 쓰고, 저장하거나 다른 곳으로 보내지 않아요."),
      h("li", {}, "약관상 회색지대일 수 있으니 본인 판단으로 켜 주세요."),
      h("li", {}, "macOS에서는 처음 한 번 키체인 접근 허용 창이 떠요. '항상 허용'을 누르면 돼요."),
    ),
    h("div", { class: "btn-row" }, ok, no),
  );
  ok.addEventListener("click", () => save({ oauth_acknowledged: true, oauth_enabled: true }));
  no.addEventListener("click", () => (notice.hidden = true));
  wrap.append(field("조회 켜기", toggle), notice);
  if (c.oauth_enabled) {
    wrap.append(
      field(
        "조회 간격",
        select(
          [2, 3, 5, 10, 15].map((m) => [String(m), `${m}분`]),
          String(c.poll_minutes),
          (v) => save({ poll_minutes: Number(v) }),
        ),
      ),
    );
    const msg = snapshot?.oauth.message;
    if (msg) wrap.append(h("p", { class: "warn-text" }, msg));
  }
  return wrap;
}

function renderSettings(): void {
  $settings.replaceChildren();
  if (!config) return;
  const c = config;
  const hours: [string, string][] = Array.from({ length: 24 }, (_, i) => [String(i), `${String(i).padStart(2, "0")}:00${i === 0 ? " (자정)" : ""}`]);
  const mode = select(
    [
      ["rolling", "재분배형 오늘 예산 (추천)"],
      ["fixed", "고정 1/7 + 이월 잔고"],
    ],
    c.mode,
    (v) => save({ mode: v as Config["mode"] }),
  );

  const back = h("button", { class: "btn" }, "← 돌아가기");
  back.addEventListener("click", () => toggleSettings(false));
  const quit = h("button", { class: "btn danger" }, "Daily Sausage 종료");
  quit.addEventListener("click", () => api.quit());

  $settings.append(
    h("div", { class: "settings-head" }, back, h("h2", {}, "설정")),
    h("p", { id: "settings-message", class: "message", hidden: true }),
    h(
      "div",
      { class: "group" },
      h("h3", {}, "오늘 예산"),
      field("하루가 바뀌는 시각", select(hours, String(c.boundary_hour), (v) => save({ boundary_hour: Number(v) })), "자정을 넘겨 작업하는 날이 많다면 새벽 4시처럼 늦춰도 좋아요."),
      field("표시 방식", mode, c.mode === "rolling" ? "하루가 바뀔 때마다 남은 양을 남은 시간으로 다시 나눠요." : "매일 1/7씩 같은 예산을 주고, 아끼거나 넘친 양은 잔고로 보여줘요."),
    ),
    statuslineBlock(),
    oauthBlock(c),
    h(
      "div",
      { class: "group" },
      h("h3", {}, "알림"),
      field("오늘 예산 80% 도달", checkbox(c.notify_today_80, (v) => save({ notify_today_80: v }))),
      field("오늘 예산 초과", checkbox(c.notify_today_100, (v) => save({ notify_today_100: v }))),
      field("5시간 창 90%", checkbox(c.notify_five_hour_90, (v) => save({ notify_five_hour_90: v }))),
    ),
    h("div", { class: "group" }, h("h3", {}, "기타"), field("로그인할 때 자동 시작", checkbox(c.autostart, (v) => save({ autostart: v })))),
    h("div", { class: "btn-row" }, quit),
    h("p", { class: "disclaimer" }, "Daily Sausage는 Claude 구독 사용량 전용 서드파티 프로그램이에요. Anthropic이 만들거나 보증한 프로그램이 아니에요."),
  );
}

async function toggleSettings(on: boolean): Promise<void> {
  showingSettings = on;
  if (on) config = await api.getConfig();
  $main.hidden = on;
  $settings.hidden = !on;
  $btnSettings.classList.toggle("active", on);
  if (on) renderSettings();
  else renderMain();
}

// ---------- wiring ----------

function applySnapshot(s: Snapshot): void {
  snapshot = s;
  if (showingSettings) {
    // Keep the form, but refresh status texts that come from the backend.
    const scroll = $settings.scrollTop;
    renderSettings();
    $settings.scrollTop = scroll;
  } else {
    renderMain();
  }
  $footer.textContent = s.model.footer;
}

$btnSettings.addEventListener("click", () => toggleSettings(!showingSettings));
$btnRefresh.addEventListener("click", async () => {
  $btnRefresh.classList.add("spin");
  await api.refresh();
  setTimeout(() => $btnRefresh.classList.remove("spin"), 800);
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (showingSettings) toggleSettings(false);
    else api.hidePopup();
  }
});

(async () => {
  await api.onSnapshot(applySnapshot);
  const s = await api.getSnapshot();
  if (s) applySnapshot(s);
  else renderMain();
  if (new URLSearchParams(location.search).get("view") === "settings") toggleSettings(true);
})();
