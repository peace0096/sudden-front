// Human-facing weapon descriptions and the primary-weapon picker of the in-game B menu.
import { WeaponKind, type WeaponStats } from "./gen/game_pb";

export const WEAPON_ORDER = [WeaponKind.RIFLE, WeaponKind.SNIPER, WeaponKind.MACHINE_GUN, WeaponKind.SMG];

interface Description {
  type: string;
  damage: string;
  speed: string;
  accuracy: string;
  fireRate: string;
}

const DESCRIPTIONS: Record<number, Description> = {
  [WeaponKind.RIFLE]: { type: "소총", damage: "보통", speed: "보통", accuracy: "보통", fireRate: "보통" },
  [WeaponKind.SNIPER]: {
    type: "저격소총",
    damage: "매우 높음",
    speed: "약간 느림",
    accuracy: "스코프 매우 높음 / 그 외 약간 낮음",
    fireRate: "볼트액션",
  },
  [WeaponKind.MACHINE_GUN]: { type: "기관총", damage: "약간 높음", speed: "매우 느림", accuracy: "낮음", fireRate: "매우 높음" },
  [WeaponKind.SMG]: { type: "기관단총", damage: "낮음", speed: "매우 빠름", accuracy: "약간 높음", fireRate: "높음" },
  [WeaponKind.PISTOL]: { type: "권총", damage: "낮음", speed: "약간 높음", accuracy: "높음", fireRate: "낮음" },
  [WeaponKind.PISTOL_PLUS]: { type: "강화 권총", damage: "약간 낮음", speed: "약간 높음", accuracy: "높음", fireRate: "3점사" },
  [WeaponKind.MAGNUM]: { type: "매그넘", damage: "매우 높음", speed: "빠름", accuracy: "높음", fireRate: "느림" },
  [WeaponKind.AKIMBO_SMG]: { type: "우지 아킴보", damage: "낮음", speed: "매우 빠름", accuracy: "약간 높음", fireRate: "매우 높음" },
  [WeaponKind.SHOTGUN]: { type: "샷건", damage: "높음", speed: "보통", accuracy: "근거리", fireRate: "낮음" },
  [WeaponKind.MISSILE]: { type: "미사일 런처", damage: "매우 높음", speed: "보통", accuracy: "폭발형", fireRate: "빠름" },
  [WeaponKind.TASER]: { type: "테이저건", damage: "없음(마비)", speed: "약간 높음", accuracy: "높음", fireRate: "보통" },
  [WeaponKind.KNIFE]: { type: "나이프", damage: "높음", speed: "높음", accuracy: "근접 전용", fireRate: "-" },
  [WeaponKind.GRENADE]: { type: "수류탄", damage: "폭발형", speed: "약간 높음", accuracy: "투척", fireRate: "목숨당 1개" },
  [WeaponKind.HAMMER]: { type: "뿅망치", damage: "한 방", speed: "매우 빠름", accuracy: "근접 전용", fireRate: "-" },
};

/** The pistol, knife and grenade everyone carries, shown under the primary picker. */
export function sidearmNote(): HTMLElement {
  const note = document.createElement("div");
  note.className = "sidearm-note";
  for (const text of [
    "1번 주무기: 위에서 선택",
    "2번 권총 (공통): 대미지 낮음 · 연사력 낮음 · 이동속도 약간 높음 · 정확도 높음 · 12/36",
    "3번 나이프 (공통): 대미지 높음 · 이동속도 높음 · 근접 공격 전용",
    "4번 수류탄 (공통): 목숨당 1개 · 던지고 3초 뒤 폭발 · 반경 5 m · 자신과 아군은 피해 없음",
  ]) {
    const line = document.createElement("div");
    line.textContent = text;
    note.append(line);
  }
  return note;
}

export function weaponTypeName(kind: WeaponKind): string {
  return DESCRIPTIONS[kind]?.type ?? "";
}

export function weaponCards(
  weapons: WeaponStats[],
  selected: WeaponKind,
  onPick: (kind: WeaponKind) => void,
): HTMLElement {
  const grid = document.createElement("div");
  grid.className = "weapon-grid";
  WEAPON_ORDER.forEach((kind, index) => {
    const stats = weapons.find((w) => w.kind === kind);
    const info = DESCRIPTIONS[kind];
    if (!stats || !info) return;
    const card = document.createElement("button");
    card.type = "button";
    card.className = kind === selected ? "weapon-card selected" : "weapon-card";
    card.setAttribute("aria-pressed", String(kind === selected));
    card.onclick = (event) => {
      event.stopPropagation();
      onPick(kind);
    };

    const head = document.createElement("div");
    head.className = "weapon-card-head";
    const title = document.createElement("strong");
    title.textContent = `${index + 1}. ${info.type}`;
    const name = document.createElement("span");
    name.textContent = stats.name;
    head.append(title, name);
    card.append(head);

    const rows: [string, string][] = [
      ["대미지", info.damage],
      ["이동속도", info.speed],
      ["탄약", `${stats.magazine}/${stats.reserve}`],
      ["정확도", info.accuracy],
      ["연사력", info.fireRate],
    ];
    for (const [label, value] of rows) {
      const row = document.createElement("div");
      row.className = "weapon-stat";
      const left = document.createElement("span");
      left.textContent = label;
      const right = document.createElement("span");
      right.textContent = value;
      row.append(left, right);
      card.append(row);
    }
    grid.append(card);
  });
  return grid;
}
