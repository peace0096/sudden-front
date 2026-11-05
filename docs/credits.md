# 크레딧

이 파일은 `tools/art/build-art.mjs`가 `client/public/art/LICENSES.json`에서 만듭니다. 손으로 고치지 말고, 레시피(`tools/art/recipes/`)를 고친 뒤 다시 실행하세요.

게임의 모델·텍스처·이펙트 이미지는 아래 팩에서 가져와 변환했습니다. 사운드는 코드로 합성하고, 그 밖의 그래픽(하늘, 이름표, 광고판 글자, 벽·바닥 텍스처 등)도 코드로 그립니다.

## 팩

| 팩 | 저작자 | 라이선스 | 출처 |
| --- | --- | --- | --- |
| Particle Pack | Kenney | CC0-1.0 (License (Creative Commons Zero, CC0)) | https://opengameart.org/content/particle-pack-80-sprites |
| Splat Pack | Kenney | CC0-1.0 (License: (Creative Commons Zero, CC0)) | https://opengameart.org/content/splat-pack |
| Toon Shooter Game Kit | Quaternius | CC0-1.0 (CC0 1.0) | https://poly.pizza/bundle/Toon-Shooter-Game-Kit-qraiSXoAru |
| Ultimate Guns Pack | Quaternius | CC0-1.0 (CC0 1.0) | https://poly.pizza/bundle/Ultimate-Guns-Pack-cpgUfI4t2F |
| Ultimate Modular Men Pack | Quaternius | CC0-1.0 (CC0 1.0) | https://poly.pizza/bundle/Ultimate-Modular-Men-Pack-ZiH8muWqwQ |
| Sudden Front procedural models | Sudden Front | CC0-1.0 (CC0 1.0 (이 저장소의 tools/art 스크립트가 만든 모델)) | tools/art/recipes/ |

CC0 팩은 표기 의무가 없지만, 저작자에게 감사하는 뜻으로 적습니다.

## 파일

### `characters/swat-blue.glb`

- 팩: Ultimate Modular Men Pack (Quaternius, CC0-1.0)
- 원본: `swat-Btfn3G5Xv4.glb`
- 바꾼 점:
  - 클립 24개 중 9개만 남기고 이름 변경 (Idle→idle, Run→run, Run_Back→runBack, Run_Left→strafeL, Run_Right→strafeR, Walk→walk, Death→death, Idle_Gun_Pointing→holdRifle, Gun_Shoot→shoot)
  - 부위 메시 4개를 스킨 메시 하나로 합침
  - Swat 재질에 팀 색 #3f6fa8을 70% 섞음
  - 미터 단위로 정규화: 머리 뼈 1.5 m, 발 y = 0, 아마추어의 회전·100배 배율을 뼈에 합침 (×0.9632)
  - 뼈 이름을 게임 표준 이름으로 바꿈 (hips·spine·chest·neck·head·clavicle/upperArm/foreArm/hand·thigh/shin/foot + L/R)
  - 삼각형 7752개를 4790개로 감량 (면별 법선을 지우고 용접, 평면 셰이딩 유지)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침, 키프레임 정리(resample), meshopt 압축

### `characters/swat-neutral.glb`

- 팩: Ultimate Modular Men Pack (Quaternius, CC0-1.0)
- 원본: `swat-Btfn3G5Xv4.glb`
- 바꾼 점:
  - 클립 24개 중 9개만 남기고 이름 변경 (Idle→idle, Run→run, Run_Back→runBack, Run_Left→strafeL, Run_Right→strafeR, Walk→walk, Death→death, Idle_Gun_Pointing→holdRifle, Gun_Shoot→shoot)
  - 부위 메시 4개를 스킨 메시 하나로 합침
  - Swat 재질에 팀 색 #6f757b을 70% 섞음
  - 미터 단위로 정규화: 머리 뼈 1.5 m, 발 y = 0, 아마추어의 회전·100배 배율을 뼈에 합침 (×0.9632)
  - 뼈 이름을 게임 표준 이름으로 바꿈 (hips·spine·chest·neck·head·clavicle/upperArm/foreArm/hand·thigh/shin/foot + L/R)
  - 삼각형 7752개를 4790개로 감량 (면별 법선을 지우고 용접, 평면 셰이딩 유지)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침, 키프레임 정리(resample), meshopt 압축

### `characters/swat-red.glb`

- 팩: Ultimate Modular Men Pack (Quaternius, CC0-1.0)
- 원본: `swat-Btfn3G5Xv4.glb`
- 바꾼 점:
  - 클립 24개 중 9개만 남기고 이름 변경 (Idle→idle, Run→run, Run_Back→runBack, Run_Left→strafeL, Run_Right→strafeR, Walk→walk, Death→death, Idle_Gun_Pointing→holdRifle, Gun_Shoot→shoot)
  - 부위 메시 4개를 스킨 메시 하나로 합침
  - Swat 재질에 팀 색 #b4483a을 70% 섞음
  - 미터 단위로 정규화: 머리 뼈 1.5 m, 발 y = 0, 아마추어의 회전·100배 배율을 뼈에 합침 (×0.9632)
  - 뼈 이름을 게임 표준 이름으로 바꿈 (hips·spine·chest·neck·head·clavicle/upperArm/foreArm/hand·thigh/shin/foot + L/R)
  - 삼각형 7752개를 4790개로 감량 (면별 법선을 지우고 용접, 평면 셰이딩 유지)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침, 키프레임 정리(resample), meshopt 압축

### `fx/decals.webp`

- 팩: Splat Pack (Kenney, CC0-1.0)
- 원본: `PNG/Double (512px)/splat19.png`, `PNG/Double (512px)/splat02.png`, `PNG/Double (512px)/splat33.png`, `PNG/Double (512px)/splat11.png`
- 바꾼 점:
  - 512 px 얼룩 4장을 256 px로 줄여 512² 아틀라스(2×2)로 모음
  - 색은 흰색, 모양은 알파로 (straight alpha)
  - WebP로 저장

### `fx/particles.webp`

- 팩: Particle Pack (Kenney, CC0-1.0)
- 원본: `PNG/muzzle_01.png`, `PNG/muzzle_02.png`, `PNG/muzzle_03.png`, `PNG/smoke_01.png`, `PNG/smoke_04.png`, `PNG/smoke_07.png`, `PNG/star_06.png`, `PNG/spark_06.png`, `PNG/fire_01.png`, `PNG/flame_01.png`, `PNG/flare_01.png`, `PNG/scorch_01.png`, `PNG/scorch_03.png`, `PNG/dirt_01.png`, `PNG/trace_01.png`, `PNG/light_01.png`
- 바꾼 점:
  - 512 px 스프라이트 16장을 256 px로 줄여 1024² 아틀라스(4×4)로 모음
  - 검은 바탕의 회색을 알파로 바꾸고 색은 흰색으로 (straight alpha)
  - WebP로 저장

### `map/barrel.glb`

- 팩: Toon Shooter Game Kit (Quaternius, CC0-1.0)
- 원본: `exploding-barrel-1orHe0kCc1.glb`
- 바꾼 점:
  - 0.9 × 1.1 × 0.9 m에 맞춤 (축별 배율 1.1465 × 1.0739 × 1.1465, Y축으로 90° 돌림), 바닥을 y = 0에
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - meshopt 압축

### `map/container.glb`

- 팩: Toon Shooter Game Kit (Quaternius, CC0-1.0)
- 원본: `shipping-container-dQXRtm5GbO.glb`
- 바꾼 점:
  - 6 × 2.6 × 2.4 m에 맞춤 (축별 배율 1.3763 × 1.2202 × 1.1483), 바닥을 y = 0에
  - 재질별로 body·frame 노드를 나눔 (body은 흰색: 인스턴스마다 색을 곱해 칠함)
  - meshopt 압축

### `map/crate.glb`

- 팩: Toon Shooter Game Kit (Quaternius, CC0-1.0)
- 원본: `crate-NlXe0ZJGUd.glb`
- 바꾼 점:
  - 1.2 × 1.2 × 1.2 m에 맞춤 (축별 배율 1.5193 × 1.5193 × 1.5193, Y축으로 90° 돌림), 바닥을 y = 0에
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - meshopt 압축

### `map/street-light.glb`

- 팩: Toon Shooter Game Kit (Quaternius, CC0-1.0)
- 원본: `street-light-nFwrlcLvM5.glb`
- 바꾼 점:
  - Light 재질에 발광색 #77735f
  - 0.4331 × 5.6 × 2.1603 m에 맞춤 (축별 배율 1.0356 × 0.9919 × 1.0356), 바닥을 y = 0에
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - meshopt 압축

### `projectiles/missile.glb`

- 팩: Sudden Front procedural models (Sudden Front, CC0-1.0)
- 원본: `tools/art/recipes/projectiles/missile.mjs`
- 바꾼 점:
  - 스크립트로 만든 모델: 몸통(반지름 0.11 m, 길이 0.65 m)·빨간 탄두(0.2 m)는 지금의 game.ts 미사일과 같은 크기, 날개 4장과 노즐을 더함
  - +Z를 향함, 재질 색을 팔레트 텍스처 하나로 모아 primitive 하나로 합침, meshopt 압축

### `weapons/grenade.glb`

- 팩: Sudden Front procedural models (Sudden Front, CC0-1.0)
- 원본: `tools/art/recipes/weapons/grenade.mjs`
- 바꾼 점:
  - 스크립트로 만든 모델: 지름 6.6 cm 몸통(어두운 띠 하나), 신관, 안전손잡이, 안전핀 고리
  - 몸통 가운데를 원점에, 축을 주먹이 쥐는 손잡이 축(앞으로 28.9°, 오른쪽으로 4.9°)에 맞춤, 신관 끝에 muzzle 노드 추가
  - 재질 색을 팔레트 텍스처 하나로 모아 primitive 하나로 합침, meshopt 압축

### `weapons/hammer.glb`

- 팩: Sudden Front procedural models (Sudden Front, CC0-1.0)
- 원본: `tools/art/recipes/weapons/hammer.mjs`
- 바꾼 점:
  - 스크립트로 만든 모델: 길이 22 cm·지름 10 cm 머리(빨간 양끝 마개, 가운데 노란 주름 여섯 겹), 파란 손잡이(빨간 이음 고리, 남색 손잡이 감개와 끝 손잡이), 끝에서 머리 가운데까지 30 cm
  - 손잡이의 쥐는 곳을 원점에, 손잡이를 주먹이 쥐는 손잡이 축(앞으로 28.9°, 오른쪽으로 4.9°)에 맞추고 머리는 손잡이와 직각으로 앞뒤로 놓음(앞 마개가 때리는 면), 머리 가운데에 muzzle 노드 추가
  - 재질 색을 팔레트 텍스처 하나로 모아 primitive 하나로 합침, meshopt 압축

### `weapons/knife.glb`

- 팩: Toon Shooter Game Kit (Quaternius, CC0-1.0)
- 원본: `knife-YnC9RcBOWs.glb`
- 바꾼 점:
  - 끝이 (0.075, 0.873, -0.482) 방향(주먹이 쥐는 손잡이 축)을 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 0.3 m로 배율 조정 (×0.213)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 그 방향의 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/machine_gun.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `assault-rifle-Bgvuu4CUMV.glb`, `bipod-Ugy8tjR3Ju.glb`
- 바꾼 점:
  - Bipod 모델을 붙임
  - 박스 탄창을 스크립트로 만들어 붙임
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 1.04 m로 배율 조정 (×0.2012)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/magnum.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `revolver-XrnLUz6kQj.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 0.3 m로 배율 조정 (×0.1578)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/missile.glb`

- 팩: Toon Shooter Game Kit (Quaternius, CC0-1.0)
- 원본: `rocket-launcher-GCqUvqleqN.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 1 m로 배율 조정 (×0.7683)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/pistol_plus.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `pistol-52kQzphmeF.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 0.24 m로 배율 조정 (×0.0981)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/pistol.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `pistol-J3i9KDQ3kt.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 0.2 m로 배율 조정 (×0.1099)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/rifle.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `assault-rifle-Bgvuu4CUMV.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 0.84 m로 배율 조정 (×0.1625)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/shotgun.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `shotgun-DcNE0HVdW8.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 1 m로 배율 조정 (×0.1729)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/smg.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `submachine-gun-7ehatxr7FY.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 0.55 m로 배율 조정 (×0.136)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/sniper.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `sniper-rifle-TKaBjAEofL.glb`
- 바꾼 점:
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 1.2 m로 배율 조정 (×0.1656)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축

### `weapons/taser.glb`

- 팩: Ultimate Guns Pack (Quaternius, CC0-1.0)
- 원본: `pistol-J3i9KDQ3kt.glb`
- 바꾼 점:
  - 재질 색 변경: Metal → #d8b52c, LightMetal → #e3c650
  - 총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 0.2 m로 배율 조정 (×0.1099)
  - 재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침
  - 총구 끝에 muzzle 노드 추가, meshopt 압축
