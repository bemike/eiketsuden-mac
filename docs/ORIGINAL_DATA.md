# 원작 데이터 가져오기 (실험적)

> **English summary** — see [the end of this page](#english-summary).

영걸전 Reloaded는 자체 제작한 라이선스 청정 기본 팩(`data/base`)만으로 완전히 동작합니다. 이 문서가 설명하는
**원작 데이터 임포터**는 KOEI의 1995년작 『삼국지 영걸전』 정품을 **직접 보유한 플레이어**가 자기 PC에서
원작 파일을 읽어 중립 형식(PNG, UTF-8 JSON)으로 바꿔 쓰게 해 주는 **선택적·실험적** 기능입니다.
OpenRCT2가 사용자의 RCT2 데이터를 읽는 방식과 같습니다.

| 구성 요소 | 위치 |
|---|---|
| 라이브러리 | `crates/hero-import` (프로브·컨테이너·텍스트·그래픽·추출) |
| 명령줄 | `hero-tools original probe` / `hero-tools original extract` |
| 게임 연동 | `eiketsuden --original <폴더>` 또는 환경 변수 `EIKETSUDEN_ORIGINAL` (네이티브 전용) |

## 1. 법적·윤리적 원칙

* **정품을 직접 보유한 경우에만** 쓰세요. 이 프로젝트는 원작 파일·디스크 이미지·ROM을 배포하지도, 내려받는
  방법을 안내하지도 않습니다.
* **읽기 전용·로컬 전용**입니다. 임포터는 설치 폴더에 아무것도 쓰지 않고(출력 폴더가 설치 폴더 안이면
  거부합니다), 아무것도 업로드하지 않습니다. 결과는 사용자가 고른 로컬 폴더에만 저장됩니다. 저장소 안에
  두려면 `.gitignore`에 등록된 `data/original/`을 쓰세요. 기본 출력 위치는 없습니다(`--out`은 필수).
* **복제 방지(copy protection)를 우회하지 않습니다.** 컨테이너가 암호화되어 있으면 감지·보고에서 멈춥니다.
  (Steam판 컨테이너 형식은 아직 모릅니다. 암호화가 확인되면 복호화기를 만들지 않고 법률 검토를 먼저 합니다.)
* **클린룸 구현**입니다. 공개된 형식 *사실*(이름·크기·구조·수치)만 조사 노트로 정리해 구현했고, 다른 프로젝트의
  코드(OpenEiketsuden — 라이선스 없음, ccz-compat-engine — GPLv3, lemonhall/LS11_encode_decode — 라이선스
  없음)는 읽거나 복사하지 않았습니다.
* **저장소와 CI에는 원작 바이트가 한 바이트도 없습니다.** 모든 테스트는 우리 인코더(LS11 인코더 포함)로 만든
  합성 데이터를 씁니다. 실제 설치본으로 하는 골든 테스트는 개발자가 환경 변수로 자기 설치본을 가리킬 때만
  돕니다.
* **추출 결과물은 공유하지 마세요.** 추출된 PNG·텍스트는 KOEI TECMO의 저작물입니다. 공유해도 되는 것은
  내용이 없는 **프로브 매니페스트**뿐입니다(5절).

## 2. 지원 판본

판본은 파일 이름·크기·헤더로 식별하며, 어느 규칙에도 맞지 않으면 추측하지 않고 `unknown`으로 보고합니다.
판정에는 항상 근거(evidence)가 함께 기록됩니다.

| 판본 | 식별 규칙 | 식별 | 추출 |
|---|---|---|---|
| 한국어 DOS/V (비스코, `korean-dos`) | `DISK1.R3I`에 EUC-KR 헤더 문자열 `DOS/V 삼국지영걸전` | 높음 | **지원** (텍스트·스프라이트) |
| 번체 중문 DOS (第三波, `chinese-dos`) | DOS/V 파일군 + `SNRnM`/`IPPAN0M`/`BAKDATA`의 텍스트가 뚜렷한 Big5 | 중간 (통계적 판정) | **지원** (텍스트·스프라이트, 실기 검증 전) |
| Steam 2017 (`steam-2017`) | `Eiketsuden1_Launcher.exe` 존재 | 높음 | **미지원** — 컨테이너 형식 미상, 매니페스트 수집 중 |
| PC-98 디스크 이미지 (`pc98-disk-images`) | D88 / Anex86 FDI·HDI 헤더 + 크기 일관성 | 높음 | **미지원** — 이미지 안의 파일 읽기는 P8 |
| 그 밖 | — | `unknown` | 거부 (`--edition`으로 강제 가능, 기록됨) |

식별은 지정한 폴더 바로 아래 파일만 봅니다. 한국어 DOS/V판은 설치 폴더 안의 `GAME` 폴더를 지정하세요
(`GAME` 하위 폴더가 보이면 프로브가 힌트를 줍니다).

## 3. 에셋 종류별 신뢰도와 현재 상태

| 에셋 | 원작 파일 | 상태 | 신뢰도 / 남은 가정 |
|---|---|---|---|
| LS11 아카이브 | 대부분의 `.R3` | 구현 | 높음. 디렉터리 체인, 마지막 항목이 파일 끝에서 끝남, 정확한 복원 길이, 입력 완전 소비를 모두 검사. `Ls10`/`Ls12` 변형은 "지원 안 함"으로 보고 |
| 6바이트 테이블 컨테이너 | `FACEDAT.R3`, `PACKGRP.R3` | 구현 | 높음. 체인·파일 끝 검사 |
| 대사·문자열 | `SNR0M`–`SNR4M.R3`, `IPPAN0M.R3` | **추출** → `text/*.json` | 높음. 섹션 수를 `SNRnD.R3` 장면 수와 교차 검증. 대사 레코드 앞의 u16 화자 번호는 바이트코드(P6) 없이는 경계를 알 수 없어 분리하지 않음. 깨끗하게 디코딩되지 않은 블록은 원본 hex를 함께 기록 |
| 팔레트 | `MAIN.EXE` 안 | **추출** → `gfx/original/palettes.json` | 높음. 9 슬롯 × 48바이트 + `80 40 20 10` 서명으로 위치 탐색(고정 오프셋 안 씀), [B][R][G] 4비트 → VGA 6비트 → 8비트. **어느 슬롯이 어느 화면용인지는 미상** — 스프라이트 PNG는 슬롯 0 사용 |
| 유닛 스프라이트·맵 칩 | `HEXBCHR`, `HEXICHR`, `HEXZCHR`, `HEXZCHP`, `HEXBCHP`, `MMAPBGPL`, `SMAPBGPL` | **추출** → `gfx/original/<파일>/<nnn>.png` | 중상. 16×16 셀·4 비트플레인·MSB=왼쪽은 문서화됨. **가정**: 플레인 p = 색 번호의 비트 p(VGA 관례), 9/16/36셀 항목은 3×3/4×4/6×6 행 우선 배치(셀 순서 미문서화), 나머지는 16열 셀 시트. 저장된 한 방향만 내보냄(반대 방향은 엔진이 좌우 반전). 색 0은 투명 |
| 얼굴 그림 | `FACEDAT.R3` (TF-DCE 압축) | **지원 안 함** (명시 보고) | 컨테이너(240개)는 검증하지만, 조사 노트에 TF-DCE의 연산 목록만 있고 비트 단위 명령 인코딩이 없어 디코더를 만들면 추측이 됨 |
| 무장·아이템 이름 | `BAKDATA.R3` | **지원 안 함** (명시 보고) | 레코드 바이트 배치가 공개되지 않음 |
| 전투·전략·도시 맵, 규칙 표, 시나리오, 음악, 세이브 | `HEXZMAP` 등 | 아직 없음 | 로드맵(7절) 참고 |

## 4. 사용법

### 4.1 준비

```sh
cargo build --release -p hero-tools      # target/release/hero-tools(.exe)
```

### 4.2 판본 확인 (프로브)

```sh
hero-tools original probe "D:/Games/영걸전/GAME" --out manifest.json
```

* 식별된 판본·신뢰도·근거, 파일 수, LS11 아카이브 검증 결과, 지원 범위를 출력합니다.
* `--out`을 주면 공유 가능한 매니페스트를 씁니다(설치 폴더 안에는 쓰지 않습니다).
* 폴더를 읽을 수 없으면 종료 코드 1, 명령줄 오류는 2입니다. 판본을 모르는 폴더도 프로브 자체는 성공(0)입니다.

### 4.3 추출

```sh
hero-tools original extract "D:/Games/영걸전/GAME" --out "D:/영걸전-원작" [--text] [--sprites] [--portraits]
```

* 종류 옵션이 없으면 모든 종류를 시도하고, 지원하지 않는 종류(얼굴·이름)는 보고만 합니다.
* 종류를 **명시했는데** 추출할 수 없으면(예: `--portraits`) 실패로 보고하고 종료 코드 1을 돌려줍니다.
  어느 종류든 오류가 나면(`partial`/`failed`) 역시 1입니다.
* `--edition korean-dos|chinese-dos`는 식별을 건너뜁니다(식별 결과는 근거로 함께 기록됩니다).
* 출력 폴더는 새 폴더·빈 폴더·이전 추출 결과(`index.json`) 중 하나여야 합니다. 이전 결과면
  `index.json`에 적힌 파일만 지우고 다시 씁니다. 다른 파일이 든 폴더는 거부합니다.

출력 구조 (미디어 오버레이):

```text
<out>/index.json                         판본, 종류별 상태(extracted/partial/failed/unsupported/missing-source),
                                         요약·주의사항·오류, 원본 파일 SHA-256, 쓴 파일 목록
<out>/gfx/original/palettes.json         MAIN.EXE 팔레트 9슬롯
<out>/gfx/original/hexbchr/000.png ...   미디어 키 original/hexbchr/000
<out>/text/snr0m.json ...                섹션 → 블록(섹션 기준 상대 오프셋, 텍스트)
```

### 4.4 게임에서 쓰기

```sh
eiketsuden --original "D:/영걸전-원작"          # 또는 EIKETSUDEN_ORIGINAL=D:/영걸전-원작
```

* 게임의 미디어 저장소(텍스처·사운드·아이콘 목록)가 **오버레이 폴더를 먼저, 그다음 데이터 팩을** 찾습니다.
  오버레이에 없는 파일은 팩에서 읽습니다. 규칙·대사 같은 팩의 텍스트 파일은 오버레이하지 않습니다.
* `index.json`이 없는 폴더(예: 설치 폴더 자체)를 지정하면 경고를 남기고 무시합니다.
* **현재 한계**: 추출물의 키(`original/...`)는 기본 팩이 쓰는 키(`portraits/liu_bei`, `units/archer_player` 등)와
  다르므로, 오버레이를 켜도 기본 게임 화면이 자동으로 원작 그림으로 바뀌지는 않습니다. 팩(모드)이 `original/...`
  키를 참조하거나, 오버레이 폴더 안에 팩과 같은 키 이름으로 파일을 두면(예: `gfx/portraits/liu_bei.png`) 그 파일이
  우선합니다. 원작 레이아웃을 기본 팩 키로 옮기는 매핑은 얼굴 디코딩(TF-DCE)과 맵 칩 매핑 이후의 과제입니다.
  이 오버레이는 미디어만 바꾸는 임시 경로이고, 장기 목표인 "원작 모드"는 기본 팩을 확장하는 팩으로 계획되어
  있습니다(8절).
* **웹 빌드는 지원하지 않습니다.** 브라우저에는 로컬 폴더를 읽는 경로가 없어 `--original`이 없습니다
  (향후 File System Access API/OPFS로 검토).

## 5. 매니페스트 공유로 돕기 (특히 Steam판)

Steam판(2017, 앱 628150)은 지금 새로 살 수 있는 유일한 판본이지만, 설치 폴더 안의 컨테이너 형식이 알려져
있지 않습니다. 보유자가 매니페스트를 공유해 주면 저작물 없이도 형식을 파악할 수 있습니다.

1. `hero-tools original probe "<Steam>/steamapps/common/Eiketsuden1" --out steam-manifest.json`
2. 파일을 열어 **파일 목록을 직접 확인**하세요. 매니페스트에는 폴더 기준 **상대 경로**, 크기, SHA-256,
   각 파일의 **첫 16바이트**(형식 식별용 매직 넘버), 컨테이너 요약(LS11 항목 수·저장/복원 길이·검증 결과,
   6바이트 테이블 항목 길이, PC-98 디스크 이미지 헤더 종류, `MAIN.EXE` 팔레트 위치)만 들어갑니다.
   복원된 데이터·텍스트·픽셀과 절대 경로(사용자 이름이 들어가는)는 들어가지 않습니다.
3. 게임 폴더가 아닌 곳(예: 홈 폴더)을 가리키면 그곳의 파일 이름이 기록되니 주의하세요. 최대 5,000개 파일,
   8단계 깊이까지만 보며 심볼릭 링크는 따라가지 않습니다.
4. 프로젝트 저장소의 이슈에 매니페스트를 첨부해 주세요. 한국어 DOS/V·번체 중문판·PC-98 이미지의 매니페스트도
   변형판 식별에 도움이 됩니다.

## 6. 검증 방식

* **구조 불변식을 오라클로**: LS11 디렉터리 체인·파일 끝 일치·정확한 복원 길이·입력 완전 소비, 6바이트 테이블
  체인, 메시지 표(첫 값 = 2 × 섹션 수, 섹션마다 NUL 종결), `SNRnM` 섹션 수 = `SNRnD` 장면 수. 위반하면 그
  종류는 정확한 오류와 함께 실패하고 부분 결과를 남기지 않습니다.
* **합성 픽스처**: 우리 LS11 인코더·6바이트 테이블 작성기·플레인 인코더·팔레트 뱅크 작성기로 만든 데이터로
  왕복·손상 입력·불변식 위반을 단위 테스트합니다.
* **골든 테스트** (`crates/hero-import/tests/golden.rs`): 자기 설치본의 데이터 폴더를 가리키면 실행됩니다.

  ```sh
  EIKETSU_ORIGINAL_DIR="D:/Games/영걸전/GAME" cargo test -p hero-import --test golden -- --nocapture
  ```

  모든 컨테이너 검증, 한국어판 공개 수치(얼굴 240·`PACKGRP` 38·`HEXBCHR` 181개와 데이터 시작 0x990,
  전투 맵 59개와 0번 56×32, 캠페인 맵 크기 공식, 장면 수 1/5/4/5/3, 프롤로그 이벤트 오프셋 표, `SNR0M`
  10,920바이트·`IPPAN0M` 37,580바이트, 블록 수 5,677/653, 팔레트 오프셋 0x38DF0), 전체 추출 성공을
  확인합니다. 같은 검사를 문서화된 형태로 만든 합성 설치본에도 돌려 CI에서 검사 코드 자체를 검증합니다.
  공개된 얼굴 0번 해시 접두어는 TF-DCE를 구현하지 않아 검사하지 않습니다. 블록 수는 다른 프로젝트의 집계라
  세는 방식이 다를 수 있습니다(불일치 시 메시지에 명시).
* **아직 실제 설치본으로 검증되지 않았습니다.** 코덱의 세부(가변 길이 코드의 정확한 구간, 역참조 거리 기준)는
  조사 노트의 서술을 해석한 것이므로, 첫 골든 테스트 실행 결과가 가장 중요한 확인 절차입니다.

## 7. 로드맵 (정직한 현황)

| 단계 | 범위 | 현황 |
|---|---|---|
| P0 프로브 | 판본 식별, 공유용 매니페스트 | **완료** (Steam·PC-98은 식별만) |
| P1 컨테이너 | LS11(+인코더), 6바이트 테이블 | **완료**. 음악용 테이블 컨테이너는 P7과 함께 |
| P2 텍스트 | `SNR?M`, `IPPAN0M` (EUC-KR/Big5) | **완료** (화자 번호 분리는 P6 필요). 이름(`BAKDATA`)은 P5 |
| P3 그래픽 | 플레인 셀·팔레트 → PNG | **부분**: 스프라이트·칩·배경 셀 완료(가정 3가지 명시), 얼굴(TF-DCE)은 미지원 |
| P4 맵 | `HEXZMAP` 59개, `MMAP`, `SMAP`/`PMAP` → 타일 맵 JSON + 참고 PNG | 미착수 (칩 매핑 불명) |
| P5 규칙·무장 | `BAKDATA` 배치 규명, `MAIN.EXE` 병종·지형·책략 표 서명 검색 | 미착수 |
| P6 시나리오 | `SNR?D` 바이트코드 → 우리 이벤트 형식으로 변환 | 미착수 |
| P7 음악 | OPL2 시퀀스 → FM 합성 | 미착수 (합성기 라이선스·크기 검토 필요) |
| P8 Steam / PC-98 | Steam 컨테이너(매니페스트 수집 후), 디스크 이미지 리더, Shift-JIS·OPN 변형 | 미착수 — **Steam 매니페스트가 선행 조건**. 암호화가 있으면 법률 검토 전 중단 |
| P9 세이브 | `ESAVE/MSAVE` 가져오기 | 선택 사항 |

## 8. 원작 모드 (계획 — 아직 구현되지 않음)

OpenRCT2가 RCT2 데이터로 게임을 보여 주듯, 장기 목표는 플레이어가 보유한 원작의 에셋으로 게임을 그리는
**원작 모드**입니다. 원작 모드는 별도 실행 경로가 아니라 **기본 팩을 확장하는 레이어드 팩**으로 설계합니다
(팩 레이어링은 [MODDING.md](MODDING.md#layered-packs-extends), 결정 기록은 [DECISIONS.md](DECISIONS.md) D8).

* **토대 (구현됨)**: `pack.toml`의 `extends = "../base"`로 팩이 다른 팩 위에 얹힙니다. 자식 팩은 자기가 가진
  파일만 적고, 규칙 파일·무장·캠페인은 자식이 적으면 부모 것을 대체하며, 전투·대사 장면은 합쳐지고(같은 id는
  자식이 우선), 미디어는 자식 폴더를 먼저, 없으면 부모 폴더를 찾습니다. `[presentation] canvas = [w, h]`로 팩의
  가상 캔버스 크기를 적을 수 있습니다(기본 480×270, 320×200..1280×800, 자식이 적지 않으면 상속).
* **계획**: 임포터(`hero-tools original ...`)가 사용자의 정품에서 읽은 결과를 기본 팩 옆의 로컬 폴더
  `data/original/`(`.gitignore`에 이미 등록)에 **팩**으로 씁니다. 그 `pack.toml`은 `extends = "../base"`와
  `[presentation] canvas = [640, 480]`(원작의 VGA 화면)을 적고, 변환에 성공한 것만 담습니다. 변환되지 않은
  나머지(규칙, 맵, 시나리오, 음악, 아직 매핑되지 않은 그림)는 체인을 통해 기본 팩에서 옵니다. 그래서 원작
  모드는 에셋 하나하나가 변환될 때마다 조금씩 원작에 가까워질 수 있습니다. 실행은 `eiketsuden --data data/original`
  형태가 될 것입니다.
* **아직 없는 것**: 임포터는 현재 4절의 미디어 오버레이만 쓰며 팩을 쓰지 않습니다. 원작 파일(16×16 4bpp 칩,
  48×48/64×64 유닛 스프라이트, 64×80 얼굴, 32–80 × 22–48 칩의 전투 맵)을 팩의 키와 규칙으로 옮기는 **매핑은
  정해지지 않았고, 추측하지 않습니다.** 얼굴(TF-DCE), 맵 칩 대응(P4), 규칙·무장 표(P5), 시나리오(P6)가 규명되는
  순서대로 팩에 들어갈 항목이 늘어납니다.
* **제약**: `extends`는 상대 경로만 허용하므로(웹 빌드와 폴더 이동을 위해) 원작 모드 팩은 기본 팩과 같은 드라이브,
  예컨대 `data/original/`에 둡니다. 브라우저는 로컬 폴더를 읽을 수 없으므로 웹 빌드에는 원작 모드가 없습니다.
  세이브는 최상위 팩의 `id`를 기억하므로 원작 모드의 세이브는 기본 팩의 세이브와 섞이지 않습니다.

---

## English summary

The **original-data importer** is an optional, experimental feature for players who **own** a copy of KOEI's
1995 *Sangokushi Eiketsuden*. The game never needs it. It reads the player's install **read-only**, uploads
nothing, never circumvents copy protection (an encrypted container stops at detection), and writes neutral files
(PNG, UTF-8 JSON) into a local folder the player chooses (never inside the install; `data/original/` is
git-ignored). It is a clean-room implementation from published format facts only; no third-party code was used,
and the repository and CI contain no original bytes (tests use synthetic fixtures from our own encoders).

* **Editions**: Korean DOS/V (identified by the `DISK1.R3I` header, extractable), Traditional-Chinese DOS
  (DOS/V family + Big5 text, medium confidence, extractable but not yet verified on a real copy), Steam 2017 and
  PC-98 disk images (identified only), anything else `unknown` (refused unless `--edition` is given).
* **Assets**: LS11 archives and 6-byte tables with full invariant checks; message text (EUC-KR/Big5 → JSON,
  scene counts cross-checked); palettes located by signature in `MAIN.EXE`; 16×16 planar sprite/chip cells → PNG
  (assumptions reported: plane order, cell order of composed sprites, palette slot). **Portraits (TF-DCE) and
  `BAKDATA.R3` names are reported as unsupported**, because the notes do not specify them well enough.
* **Usage**: `hero-tools original probe <dir> [--out manifest.json]`, then
  `hero-tools original extract <dir> --out <overlay> [--text] [--sprites] [--portraits]`, then
  `eiketsuden --original <overlay>` (or `EIKETSUDEN_ORIGINAL`; native builds only). Media keys are looked up in
  the overlay first, then in the pack. The extracted keys live under `original/...` and do not replace the base
  pack's own keys automatically yet.
* **Help wanted**: run `probe` on a Steam install (`steamapps/common/Eiketsuden1`) and attach the manifest to an
  issue. A manifest contains relative paths, sizes, SHA-256, the first 16 bytes of each file and container
  summaries — no game content, no absolute paths.
* **Verification**: `EIKETSU_ORIGINAL_DIR=<data folder> cargo test -p hero-import --test golden` checks the
  published known answers on a real install. The decoders have **not yet been run on a real install**.
* **Roadmap**: P4 maps, P5 rules/officer tables, P6 scenario bytecode, P7 OPL2 music, P8 Steam/PC-98 — all open.
* **Original mode (planned, not implemented)**: the goal is a pack the importer writes to `data/original/`
  (git-ignored) with `extends = "../base"` and `[presentation] canvas = [640, 480]`, holding only what was
  converted from the player's copy; everything else keeps coming from the base pack through the layered-pack
  chain (`extends`, which exists: see MODDING.md "Layered packs"), so the mode can grow asset by asset. The
  importer does not write such a pack yet, and no mapping from original files to the pack's keys has been
  decided. `extends` is relative only, so the pack lives next to the base pack; there is no original mode on
  the web.
