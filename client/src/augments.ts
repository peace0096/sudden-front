// Human-facing text for augment cards. Their rules live on the server.
import { Augment, AugmentGrade } from "./gen/game_pb";

export interface AugmentInfo {
  grade: AugmentGrade;
  name: string;
  description: string;
}

export const AUGMENTS: Record<number, AugmentInfo> = {
  [Augment.INFINITE_AMMO]: { grade: AugmentGrade.SILVER, name: "무한 장전", description: "장전할 수 있는 탄약이 무한입니다. 수류탄은 늘지 않습니다." },
  [Augment.SOUL_DRAIN]: { grade: AugmentGrade.SILVER, name: "영혼 흡수", description: "적을 처치할 때마다 50 회복합니다. 최대 HP를 넘어서 회복할 수 없습니다." },
  [Augment.PISTOL_UPGRADE]: { grade: AugmentGrade.SILVER, name: "권총 업그레이드", description: "권총의 대미지와 연사력이 오르고 3점사로 발사되며, 총구가 위로 튀지 않습니다. 탄약 12/120." },
  [Augment.QUICK_RELOAD]: { grade: AugmentGrade.SILVER, name: "빠른 준비", description: "장전속도가 2배 빨라집니다." },
  [Augment.SHOTGUN]: { grade: AugmentGrade.GOLD, name: "샷건", description: "2번 무기가 펌프액션 샷건이 됩니다. 발사 시 6탄환, 탄약 6/36, 한 발씩 장전하며 장전 도중 사격 가능." },
  [Augment.REST]: { grade: AugmentGrade.GOLD, name: "때로는 휴식이", description: "마지막 전투 후 4초가 지나고 앉아 있으면 최대 HP까지 서서히 회복합니다." },
  [Augment.SHIELD]: { grade: AugmentGrade.GOLD, name: "방패병", description: "권총을 들었을 때 HP 1200의 방패가 내 앞에 부착됩니다. 부활할 때 새 방패를 받습니다." },
  [Augment.SMALL_HEAD]: { grade: AugmentGrade.GOLD, name: "작은 머리", description: "머리의 크기가 50% 감소합니다." },
  [Augment.WALLHACK]: { grade: AugmentGrade.PRISM, name: "투과", description: "적이 벽 너머로 보입니다." },
  [Augment.POSSESSION]: { grade: AugmentGrade.GOLD, name: "빙의", description: "적을 처치할 때마다 HP가 150으로 초기화되고 주무기가 죽인 적의 주무기로 교체됩니다." },
  [Augment.MAGNUM]: { grade: AugmentGrade.GOLD, name: "매그넘 리볼버", description: "2번 무기가 매그넘 리볼버가 됩니다. 대미지 매우 높음, 탄퍼짐 없음, 위로 튀는 반동 매우 큼, 연사력 느림, 이동속도 빠름, 탄약 6/30." },
  [Augment.BIG_HEAD]: { grade: AugmentGrade.GOLD, name: "대두 바이러스", description: "처치에 기여할 때마다 처치된 적의 머리 크기가 25%씩 커집니다. 합연산으로 누적되며 최대 3배까지 커집니다." },
  [Augment.DOUBLE_TIME]: { grade: AugmentGrade.GOLD, name: "더블 타임", description: "무작위 증강 2개를 얻습니다. 같은 무기 칸을 바꾸는 증강은 함께 나오지 않습니다." },
  [Augment.VEST]: { grade: AugmentGrade.SILVER, name: "방탄조끼", description: "최대 HP가 200으로 증가합니다." },
  [Augment.AKIMBO]: { grade: AugmentGrade.PRISM, name: "우지 아킴보", description: "기관단총을 선택하면 양손에 듭니다. 장탄수와 연사력이 2배가 됩니다. (B키로 기관단총 선택)" },
  [Augment.NO_RECOIL]: { grade: AugmentGrade.PRISM, name: "무반동", description: "총기의 반동과 탄퍼짐이 모두 사라집니다. 대신 장전 시간이 2배로 늘어납니다." },
  [Augment.BORROW]: { grade: AugmentGrade.PRISM, name: "빌려쓰기", description: "장전 모션을 스킵하여 즉시 장전합니다." },
  [Augment.MISSILE]: { grade: AugmentGrade.PRISM, name: "미사일 런처", description: "1번 무기가 연속 발사 미사일 런처가 됩니다. 넓은 히트 범위, 폭발형 대미지 매우 높음, 연사력 빠름, 탄약 9/36." },
  [Augment.TASER]: { grade: AugmentGrade.SILVER, name: "테이저건", description: "2번 무기가 테이저건이 됩니다. 대미지는 없지만 맞은 적은 5초간 마비됩니다. 연사력 보통, 탄약 1/6." },
};

export const GRADE_NAMES: Record<number, string> = {
  [AugmentGrade.SILVER]: "실버",
  [AugmentGrade.GOLD]: "골드",
  [AugmentGrade.PRISM]: "프리즘",
};

export const GRADE_CLASSES: Record<number, string> = {
  [AugmentGrade.SILVER]: "silver",
  [AugmentGrade.GOLD]: "gold",
  [AugmentGrade.PRISM]: "prism",
};
