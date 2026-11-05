# tools/art

외부 에셋 팩을 받고, 재고, 게임용 파일로 변환하는 스크립트입니다. 게임 코드는 이 폴더를 쓰지 않습니다.

- 받은 팩은 저장소 루트의 `art-src/<팩 id>/`에 둡니다(gitignore). 신뢰하지 않는 데이터로 다루며, 그 안의 파일을 실행하거나 코드로 불러오지 않고 GLB·glTF·PNG·JSON을 데이터로만 읽습니다.
- 런타임 에셋은 `build-art.mjs`가 `client/public/art/`에 만들고, `client/src/game/art/generated.json`, `client/public/art/LICENSES.json`, `docs/credits.md`도 함께 씁니다. `client/src/game/art/manifest.json`은 손으로 고치며, `build-art.mjs`가 만든 파일과 맞는지 검사합니다.
- 후보는 `client/public/art/_candidates/`에 둡니다(gitignore). `check-art.mjs`가 건너뛰고 production 빌드가 지우므로 커밋도 배포도 되지 않습니다.

## 에셋을 바꾸는 순서

1. `sources.json`에 팩을 추가하고 `node fetch.mjs --record <팩 id>`로 받은 뒤, 팩의 라이선스를 읽고 `license`·`licenseEvidence`를 적습니다. CC0, 또는 `attribution`(표기 문구)을 적은 CC-BY 3.0/4.0만 빌드합니다. 그 밖의 라이선스는 사용자 승인 없이 쓰지 않습니다(AGENTS.md 규칙 7).
2. `recipes/`에 출력 파일 하나당 레시피 하나를 추가하거나 고칩니다.
3. `node build-art.mjs`로 만들고, 실패한 `manifest:` 줄이 있으면 출력된 값으로 `client/src/game/art/manifest.json`을 고칩니다.
4. `client/`에서 `npm run build`(check-art의 등록·라이선스·예산 검사)와 `node e2e/art.mjs`로 확인합니다. `node gallery-shots.mjs --final`이 `?art` 페이지 사진을 `client/e2e-shots/art-final/`에 남깁니다.

## 준비

Node.js 22.12 이상. 이 폴더에서 한 번:

```
npm ci
```

## 스크립트

### fetch.mjs — 팩 받기

```
node fetch.mjs [--record] [팩 id ...]
```

`sources.json`의 팩을 `art-src/<id>/`에 받습니다. 압축 파일은 `download/`에 두고 `content/`에 풀며, Poly Pizza처럼 파일 단위로 받는 팩은 `content/`에 바로 둡니다. 받은 파일마다 SHA-256을 `sources.json`과 비교해 다르면 그 팩을 실패로 처리하고, 예전에 받아 둔 파일·`content/`·`fetched.json`을 지웁니다. 이미 받은 팩은 건너뛰므로 여러 번 실행해도 됩니다.

- 팩 id는 slug, 압축·파일 이름은 경로가 없는 이름이어야 하고, 모든 경로는 자기 폴더 안에 있어야 합니다(`lib/paths.mjs`). `recipes/` 같은 곳을 가리키는 값은 쓰기 전에 거부합니다.

- 압축은 `art-src/<id>/content/` 밖을 가리키는 경로(`..`, 절대 경로)가 있으면 풀지 않고, `__MACOSX`는 버립니다.
- `--record`는 해시가 비어 있는 새 출처에만 씁니다. 받은 파일의 라이선스를 읽은 뒤 그 해시를 고정합니다.
- 실행 후 팩 안의 `License.txt` 같은 파일 경로를 출력합니다.

### inspect.mjs — 모델 재기

```
node inspect.mjs [--json | --summary] [--root <폴더>]... <파일 또는 폴더> ...
```

GLB·glTF마다 노드 트리와 뼈, 스킨과 관절 수, 클립(이름·길이), 메시별·전체 삼각형과 primitive 수, 재질(종류·색·텍스처), 경계 상자(미터)를 출력합니다. 스킨이 있는 모델은 관절로 정점을 옮겨 rest pose 기준으로 잽니다.

- 머리 뼈가 있는 캐릭터는 머리 꼭대기를 1.76 m(서버 머리 히트박스 꼭대기)에 맞춘 배율과, 그때의 목·머리 관절 높이, 턱 높이(머리 뼈에 50% 이상 붙은 정점의 최저점), 등신(1.76 ÷ 머리 높이), 바라보는 방향, 앉기(crouch) 클립을 출력합니다.
- 다른 뼈들에서 멀리 떨어진 뼈에 붙은 부품(예: 리볼버의 스피드로더를 수백 m 아래에 숨겨 둔 경우)은 `fit` 상자에서 뺍니다. `bounds`는 모든 정점입니다.
- `--summary`는 Markdown 표 한 줄씩, `--json`은 모든 측정값을 출력합니다.
- 외부 텍스처·버퍼는 `--root` 폴더 안에서만 읽습니다(기본값 `art-src/`, `client/public/art/`).

### stage-candidates.mjs — 후보 올리기

```
node stage-candidates.mjs [--fbx2gltf <FBX2glTF 경로>] [후보 id ...]
```

`candidates.json`의 후보를 `art-src/`에서 `client/public/art/_candidates/`로 복사·변환하고, 결과를 다시 재서 `_candidates/index.json`을 씁니다. id를 주지 않으면 폴더를 비우고 전부 다시 만듭니다.

- `sources.json`에서 `spdx`가 `CC0-1.0`인 팩과 파일만 올립니다. `excluded`가 있는 팩은 거부합니다.
- 모델은 외부 텍스처를 넣은 GLB 하나로 저장합니다. 후보별로 클립 일부만 남기기(`keepClips`), 재질 이름으로 색 칠하기(`recolor`), 다른 모델 붙이기(`kitbash`), 삼각형 줄이기(`simplify`)를 합니다.
- 이미지는 WebP로 바꿉니다. `luminanceAlpha`는 검은 바탕의 회색 스프라이트를 흰색 + 밝기 알파로 바꿉니다.
- FBX 후보는 FBX2glTF 실행 파일이 있어야 합니다(`--fbx2gltf` 또는 환경 변수 `FBX2GLTF`). 없으면 건너뜁니다. 저장소 의존성이 아니므로 저장소 밖에 설치합니다.

  ```
  npm install --prefix <임시 폴더> --ignore-scripts fbx2gltf@0.9.7-p1
  # macOS: <임시 폴더>/node_modules/fbx2gltf/bin/Darwin/FBX2glTF (x86_64, Apple Silicon은 Rosetta 필요)
  ```

  변환 결과는 `art-src/<팩 id>/converted/`에 둡니다.

### build-art.mjs — 런타임 에셋 빌드

```
node build-art.mjs [--check]
```

`recipes/` 아래 레시피(출력 파일 하나당 하나)를 모두 실행해 `client/public/art/`에 GLB·WebP를 쓰고, 파일마다 해시·크기·삼각형·이미지 크기를 `generated.json`에, 팩·원본 경로·바꾼 점을 `LICENSES.json`에, 같은 내용을 한국어로 `docs/credits.md`에 씁니다. 레시피가 쓰는 폴더 안에서 더는 만들지 않는 파일은 지웁니다.

- `art-src/`만 읽고 같은 입력이면 같은 바이트를 씁니다. `--check`는 아무것도 쓰지 않고 다시 빌드해, 디스크의 파일·기록과 하나라도 다르면 실패합니다.
- 끝에 `manifest.json`을 검사합니다: 매니페스트가 가리키는 파일과 빌드한 파일이 서로 같아야 하고, `fx` 칸이 아틀라스와, 캐릭터의 `forwardYaw`·클립과 무기의 `tp`(holdRifle 자세에서 오른 주먹 구멍, 곧 가운데·약지·새끼손가락이 감싸는 원의 중심에 놓인 손잡이)가 리그와 맞아야 합니다. 손잡이 축(`gripAxis`, 수직에서 앞으로 28.9°·오른쪽 4.9°)도 `rig:` 줄에 출력합니다. 다르면 넣어야 할 값을 출력하고 실패합니다.
- 원본을 쓰기 전에 `sources.json`에 고정한 SHA-256(파일 팩은 파일마다, 압축 팩은 압축 파일)을 다시 확인하고, 다르면 "run fetch.mjs"와 함께 실패합니다. `LICENSES.json`에는 잰 해시를 적습니다. `generated.json`에는 파일마다 전체 `sha256`과 `?v=`용 앞 16자 `hash`를 적고, `check-art.mjs`가 이 값으로 런타임 파일의 내용을 잠급니다.
- 레시피는 `{ output, pack, sources, build(ctx) }`를 내보냅니다. `ctx.file(경로)`는 `art-src/<pack>/content/` 안의 파일 경로를 주며, 허용된 라이선스가 아니거나 팩 밖을 가리키면 거부합니다. kitbash로 붙이는 부품도 하나하나 라이선스를 확인합니다. `pack: "procedural"`은 이 저장소 스크립트가 처음부터 만드는 모델(미사일 탄)입니다.
- `lib/`: `character.mjs`(리그 정규화·뼈와 클립 이름·감량·팀 색), `weapon.mjs`(총구 −Z·손잡이 원점·`muzzle` 노드·길이. 나이프처럼 손잡이 축으로 쥐는 무기는 `turn`으로 손잡이를 주먹 축에 맞추고 `tipAxis`로 칼끝을 찾음), `kit.mjs`(서버 상자 크기에 맞춤·인스턴스 색칠용 노드 분리), `kitbash.mjs`(다른 모델과 스크립트 상자를 붙임), `normalize.mjs`(정적 베이크·스킨 리그 베이크·감량), `atlas.mjs`(흰색 + 알파 아틀라스), `credits.mjs`, `shapes.mjs`(스크립트 도형), `scene.mjs`(팔레트·join·meshopt 저장), `sources.mjs`(라이선스·고정 해시 확인), `paths.mjs`(id·이름·경로 검증), `math.mjs`.

### gallery-shots.mjs — 사진첩

```
node gallery-shots.mjs [--final] [--swiftshader] [--only <구역,...>] [URL]
```

헤드리스 Google Chrome(`playwright-core`, `channel: "chrome"`)으로 `?art&candidates` 갤러리를 구역별로 찍어 `client/e2e-shots/art-gallery/`에 PNG와 `index.html`(설명이 달린 사진첩)을 씁니다. 셰이딩(Lambert/Toon), 톤 매핑(ACES/Neutral), 팀 색(RED/BLUE), 동작(대기/달리기/앉기)을 바꿔 가며 찍습니다. `--final`은 런타임 에셋을 보여 주는 `?art` 페이지를 클립을 바꿔 가며 찍어 `client/e2e-shots/art-final/`에 씁니다.

- URL을 주지 않으면 `client/`에서 Vite 개발 서버를 5190번부터 빈 포트에 띄우고 끝나면 끕니다. production 빌드에는 후보가 없으므로 개발 서버가 필요합니다.
- 기본은 GPU로 그립니다. `--swiftshader`는 e2e처럼 소프트웨어로 그립니다.
- `--only characters,arms`처럼 일부 구역만 `client/e2e-shots/art-gallery-only/`에 찍습니다.
- 콘솔 오류가 있으면 종료 코드 1입니다. 개발 서버에 없는 favicon 요청은 세지 않습니다.

## 데이터 파일

- `sources.json`: 팩마다 `id`, `title`, `author`, `page`(출처 페이지), `license`(라이선스 파일이나 페이지의 문구 그대로), `spdx`, `licenseUrl`, `licenseEvidence`(어디서 읽었는지), `retrieved`, 그리고 `download`·`archive`·`sha256`(압축 하나) 또는 `files`(파일마다 `download`·`license`·`sha256`). CC0이 아닌 팩에는 `excluded`에 이유를 적습니다.
- `candidates.json`: 후보마다 `category`(character·gun·map·fx·texture), `role`(총은 WeaponKind 이름, 맵은 container·crate·barrel 같은 종류), `pack`, `source`(`art-src/<pack>/content/` 안의 경로), `out`(`_candidates/` 안의 경로)과 위의 변환 옵션. 총은 `length`(갤러리에서 맞출 길이, m), 맵 조각은 `target`(서버 충돌 상자 [길이, 높이, 폭], m), 캐릭터는 `uniform`(팀 색을 섞을 재질 이름)을 씁니다.
