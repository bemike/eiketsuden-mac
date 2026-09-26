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
| 한국어 DOS/V (비스코, `korean-dos`) | `DISK1.R3I`의 **일본어 DOS/V 디스크 헤더**(Shift-JIS `DOS/V 三國志英傑伝`) + DOS/V 파일군 + 메시지 파일이 한국어 EUC-KR(2바이트 쌍의 80 % 이상이 한글 영역, 한글 500자 이상). 조사 노트가 말한 EUC-KR 헤더 `DOS/V 삼국지영걸전`도 대체 규칙으로 유지(실물에서는 못 봄) | 높음 | **지원** (텍스트·스프라이트·얼굴) |
| 번체 중문 DOS (第三波, `chinese-dos`) | DOS/V 파일군 + `SNRnM`/`IPPAN0M`/`BAKDATA`의 텍스트가 뚜렷한 Big5 | 중간 (통계적 판정) | **지원** (텍스트·스프라이트·얼굴, 실기 검증 전) |
| Steam 2017 (`steam-2017`) | `Eiketsuden1_Launcher.exe` 존재 | 높음 | **미지원** — 컨테이너 형식 미상, 매니페스트 수집 중 |
| PC-98 디스크 이미지 (`pc98-disk-images`) | D88 / Anex86 FDI·HDI 헤더 + 크기 일관성 | 높음 | **미지원** — 이미지 안의 파일 읽기는 P8 |
| 그 밖 | — | `unknown` | 거부 (`--edition`으로 강제 가능, 기록됨) |

검증한 한국어판 사본은 일본어 DOS/V판 위에 만든 현지화판입니다. `DISK1.R3I`에는 일본어판 헤더
(`DOS/V 三國志英傑伝 ﾃﾞｨｽｸ1 Ver 1.00 Rel 1.00`, Shift-JIS, `(C)(P) 1995 KOEI`)가 그대로 있고, 대사 파일만
EUC-KR 한글입니다. 그래서 헤더만으로는 언어를 정하지 않고 텍스트로 정합니다. 일본어 헤더에 한국어가 아닌
텍스트가 든 폴더(일본어 DOS/V판 — 본 적 없음)는 `unknown`으로 남깁니다.

식별은 지정한 폴더 바로 아래 파일만 봅니다. 한국어 DOS/V판은 설치 폴더 안의 `GAME` 폴더를 지정하세요
(`GAME` 하위 폴더가 보이면 프로브가 힌트를 줍니다).

## 3. 에셋 종류별 신뢰도와 현재 상태

| 에셋 | 원작 파일 | 상태 | 신뢰도 / 남은 가정 |
|---|---|---|---|
| LS11 아카이브 | 대부분의 `.R3` | 구현, **실물 검증** | 높음. `LS11`(디렉터리 빅엔디언)과 `Ls11`(리틀엔디언, `OPGRP`/`END1GRP`/`END2GRP`) 모두. 디렉터리 체인, 마지막 항목이 파일 끝에서 끝남, 정확한 복원 길이, 입력 완전 소비를 모두 검사. 한국어판 24개 중 23개가 전부 통과, `OPGRP.R3`는 검증한 사본이 손상(9.2절). `Ls10`/`Ls12` 변형은 "지원 안 함"으로 보고(실물에는 없음) |
| 6바이트 테이블 컨테이너 | `FACEDAT.R3`, `PACKGRP.R3` | 구현, **실물 검증** | 높음. 오프셋은 데이터 영역 기준(0번 = 0), 항목 수는 파일에 없어 체인이 파일 끝에서 끝나는 유일한 수로 구함(3.1절) |
| 대사·문자열 | `SNR0M`–`SNR4M.R3`, `IPPAN0M.R3` | **추출** → `text/*.json` | 높음. 섹션 수를 `SNRnD.R3` 장면 수와 교차 검증. 대사 레코드 앞의 u16 화자 번호는 바이트코드(P6) 없이는 경계를 알 수 없어 분리하지 않음. 깨끗하게 디코딩되지 않은 블록은 원본 hex를 함께 기록 |
| 팔레트 | `MAIN.EXE` 안 | **추출** → `gfx/original/palettes.json`, **실물 검증** | 높음. 9 슬롯 × 48바이트 + `80 40 20 10` 서명으로 위치 탐색(고정 오프셋 안 씀), [B][R][G] 4비트. 한국어판은 0x38DF0. 게임은 **슬롯을 실행 중에** 시나리오·맵 데이터로 고르므로, 추출기는 눈으로 확인한 슬롯을 아카이브별로 씀(9.3절) |
| 유닛 스프라이트·맵 칩·전투 UI 아이콘 | `HEXBCHR`, `HEXICHR`, `HEXZCHR`, `HEXZCHP`, `HEXBCHP`, `MMAPBGPL`, `SMAPBGPL`, `HEXGRP`(0번) | **추출** → `gfx/original/<파일>/<nnn>.png`, `sheets/<파일>.png`, `sprites.json`, **실물 검증(눈으로)** | 높음. 16×16 셀·4 비트플레인·MSB=왼쪽, 플레인 p = 색 비트 p, 셀은 행 우선 — 모두 실물 렌더링으로 확인. 항목 크기별 배치와 항목 묶음(병종·효과)은 9.4절. 저장된 한 방향만 내보냄(반대 방향은 엔진이 좌우 반전). 색 0은 투명 |
| 얼굴 그림 | `FACEDAT.R3` (TF-DCE 압축) | **추출** → `gfx/original/facedat/<nnn>.png`, **실물 검증(눈으로)** | 높음. `TFDED.COM`을 정적으로 읽어 만든 디코더(3.1절). 한국어판 240개 모두 입력을 정확히 소비하고 64×80(2560바이트)을 내며, 눈으로 확인한 결과 모두 알아볼 수 있는 얼굴. 얼굴은 색 0–7만 쓰고(플레인 3은 항상 0) 이 8색은 팔레트 슬롯 4를 뺀 8개 슬롯에서 같으므로(9.3절) 슬롯 선택의 영향이 없음(추출기는 슬롯 0). 색 0은 불투명 |
| 공통 화면·삽화 | `PACKGRP.R3` (TF-DCE 압축) | 디코딩만 (추출 종류 없음) | 높음. 38개 모두 정확히 소비. 조사 노트의 "16×16 공통 타일"이 아니라 640×400 화면 틀 2개, 512×320 창 1개, 224×144 사건 삽화 31개, KOEI 로고·문구 3개, 176×112 대리석 무늬 1개. 16색을 쓰는 3개(1·2·37번)는 어느 팔레트 슬롯이 맞는지 **미확인** |
| 오프닝·엔딩 그림 | `OPGRP`, `END1GRP`, `END2GRP` | **지원 안 함** (컨테이너만 검증) | 전체 화면 한 장이 아니라 `NPK016` 압축 그림(코덱 미해독)과 크기 정보가 없는 packed planar 그림·1비트 마스크의 묶음. 크기는 `OPEN.EXE`/`END.EXE` 코드에 있음(9.5절) |
| `MARK.R3`, `SSCCHR1/2.R3` | | **지원 안 함** | 배치 미해독(9.5절) |
| 무장·아이템 이름 | `BAKDATA.R3` | **지원 안 함** (명시 보고) | 레코드 바이트 배치가 공개되지 않음 |
| 전투 맵·전투 장면 배경·캠페인 맵·도시/궁궐 화면 | `HEXZMAP`, `HEXBMAP`, `MMAP`, `SMAP`, `PMAP` (+ `MAIN.EXE` 표) | **추출** → `maps/*.json`, `gfx/original/maps/...`, **실물 검증(눈으로)** | 높음. 칩 뱅크 구성·지형 코드·이름·크기를 `MAIN.EXE`의 읽는 코드로 확인(9.6절). 58개 전투 맵 모두 칩이 뱅크 안에 있고 눈으로 본 결과 강·숲·성·다리·마을이 제자리. 팔레트 슬롯은 게임이 실행 중에 고르므로 추출기는 슬롯 1(도시 0, 궁궐 2)을 씀. `SMAP`/`PMAP` 물체 목록 `(id, x, y)`의 의미는 **미확인** |
| 규칙 표, 시나리오, 음악, 세이브 | `MAIN.EXE`, `SNR?D` 등 | 아직 없음 | 로드맵(7절) 참고 |

### 3.1 6바이트 테이블과 TF-DCE 압축 (직접 분석)

아래는 한국어 DOS/V판의 `TFDED.COM`(1,985바이트, "TF-DCE 5.11", `int 62h` 상주 드라이버)과 `MAIN.EXE`의
호출부를 정적으로 역어셈블해 확인한 사실입니다. 다른 구현은 보지 않았습니다.

**6바이트 테이블** (`FACEDAT.R3`, `PACKGRP.R3`): `N × [u32le 오프셋][u16le 길이]` 뒤에 데이터가 옵니다.
오프셋은 **테이블 끝(데이터 영역) 기준**이라 0번 항목은 0이고, 항목이 빈틈없이 이어져 마지막 항목이 파일 끝에서
끝납니다. `N`은 파일에 없습니다. `MAIN.EXE`는 `FACEDAT.R3`의 테이블 크기 `0x5A0`(240 × 6)을 코드에 박아 두고
`항목 번호 × 6`에서 오프셋·길이를 읽은 뒤 `0x5A0 + 오프셋`으로 이동합니다. 임포터는 체인이 파일 끝과 정확히
만나는 `N`을 찾습니다(`N × 6 + 체인 끝`은 `N`에 대해 순증가하므로 그런 `N`은 많아야 하나). `PACKGRP.R3`은 38개,
데이터 시작 `0xE4`입니다.

**호출 규약**: `MAIN.EXE`에는 `int 62h` 호출이 한 곳 있고, 그 도우미는 매개변수 블록
`[x/8, y, 데이터 세그먼트, 데이터 오프셋, 플래그, 모드]`를 넘깁니다. 호출자는 하나이며 플래그 1(네 번째 플레인
사용), 모드 0(해제)으로 부릅니다. 드라이버는 VGA 메모리(한 줄 80바이트, 화면 y+40줄 위치)에 플레인별로 직접
씁니다. 모드 2는 16바이트 서명 `TF-DCE 5.11 1994` 확인이고 이 게임 파일에는 쓰이지 않습니다.

**헤더**

| 바이트 | 뜻 |
|---|---|
| `u8 L` | 헤더 길이. 뒤따르는 `L-1`바이트는 건너뜀(알려진 파일은 모두 `02 'T'`) |
| `u8` | 가로 바이트 수 `W` (픽셀 = 8W; 얼굴 8) |
| `u16le` | 세로 줄 수 `H` (얼굴 80) |
| `u16le` | 플레인 방식: 처리하는 플레인마다 4비트, 낮은 니블부터 |
| `u8` | 플레인 순서: 2비트씩 4개, 낮은 비트부터(처리 순서의 플레인 번호) |
| `u8` | 플래그: `0x40` = 32바이트 블록(팔레트로 보이나 드라이버가 건너뜀), `0x20` = 사전 `u8 개수` + `개수 × u16le` |

플레인 방식: `0` = 플레인 3이면 0으로 지움(다른 플레인이면 손대지 않음 — 임포터는 오류), `1` = 다음 바이트로
채움, `2` = 다음 바이트가 가리키는 순서 위치의 플레인을 복사, `3` = 명령 스트림. 한국어판은 모두 `0x0333`
(플레인 3 = 0), `0x3333`, `0x0223`이고 플레인 3은 항상 마지막에 처리됩니다.

**명령 스트림**: 한 플레인(`W` 바이트 열 × `H` 줄)을 **열 단위 뱀 순서**로 한 바이트씩 씁니다(0열 위→아래,
1열 아래→위, …). 마지막 바이트를 쓰는 순간 그 플레인의 스트림이 끝나며 진행 중이던 명령은 버려집니다.
`n` = 명령의 낮은 니블, `b` = 다음 바이트.

| 명령 | 동작 |
|---|---|
| `00`–`1F` | 사전 단어 `d`: 명령 `d & 0xFF`, 그 바이트 인자는 스트림 대신 `d >> 8` |
| `2n b` | 뱀 순서로 `b`칸 앞에서 `n+3`바이트 복사(겹침 허용) |
| `3n b` | 순서 위치 `n>>2` 플레인의 같은 자리에서 `b+2`바이트, `n&3`: 0 그대로, 1 NOT, 2 오른쪽 1비트 회전, 3 왼쪽 1비트 회전 |
| `4n b` | 같은 줄 `n+1`바이트 열 왼쪽에서 `b+2`바이트 복사 |
| `5n` / `9n` | 다른 플레인 같은 자리 값 AND 마스크(아래). `9n`은 바이트마다 마스크를 왼쪽 회전(`55`/`AA`는 1비트, 나머지 2비트 — 디더) |
| `6n b1 b2` | `b1 b2` 쌍을 `n+1`번 |
| `7n` | 리터럴 `n+1`바이트 (쓰는 만큼만 읽음) |
| `8n b` | `b`의 낮은 니블 `L`, 높은 니블 `H`로 `L·0x11`, `H·0x11` 쌍을 `n+1`번 |
| `A0`–`FF b` | `b`를 `명령 - 0x9E`(2–97)번 |

마스크: 최근 마스크 4개를 `00 FF 55 AA`로 시작해(이미지마다 초기화) 유지합니다. `n>>2 < 3`이면 원본은 순서 위치
`n>>2`, 마스크는 최근 `n&3`번이고 쓴 뒤 바로 앞 칸과 자리를 바꿉니다(한 칸 앞으로). `n>>2 = 3`이면 원본은 순서
위치 `n&3`, 다음 바이트가 새 마스크로 2번 칸에 들어가고(기존 2번은 3번으로), 그다음이 개수 바이트 `b`(`b+2`).
사전 단어는 낮은 바이트의 높은 니블로 분기합니다: `2` 뱀 순서 복사(인자 = 거리), `3` 플레인 변환, `4` 열 복사,
`5`/`9` 마스크 복사(인자 = 개수, 선택자 3은 새 마스크 없이 순서 위치 3), `7` 리터럴 1바이트(인자), `8` 니블 쌍,
`A`–`F` 인자로 채우기, `0`·`1`·`6` 아무것도 안 함.

출력은 플레인 우선 4bpp 플레인 형식(플레인 p = 색 번호 비트 p, 한 줄 `W`바이트)이며 `planar::decode`로 읽습니다.
임포터는 입력 완전 소비, 아직 디코딩되지 않은 플레인 읽기, 플레인 밖을 가리키는 역참조를 모두 오류로 봅니다
(한국어판 278개 이미지 중 해당 사례 없음).

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
hero-tools original extract "D:/Games/영걸전/GAME" --out "D:/영걸전-원작" [--text] [--sprites] [--portraits] [--maps]
```

* 종류 옵션이 없으면 모든 종류를 시도하고, 지원하지 않는 종류(이름)는 보고만 합니다.
* 종류를 **명시했는데** 추출할 수 없으면 실패로 보고하고 종료 코드 1을 돌려줍니다.
  어느 종류든 오류가 나면(`partial`/`failed`) 역시 1입니다.
* `--edition korean-dos|chinese-dos`는 식별을 건너뜁니다(식별 결과는 근거로 함께 기록됩니다).
* 출력 폴더는 새 폴더·빈 폴더·이전 추출 결과(`index.json`) 중 하나여야 합니다. 이전 결과면
  `index.json`에 적힌 파일만 지우고 다시 씁니다. 다른 파일이 든 폴더는 거부합니다.

출력 구조 (미디어 오버레이):

```text
<out>/index.json                         판본, 종류별 상태(extracted/partial/failed/unsupported/missing-source),
                                         요약·주의사항·오류, 원본 파일 SHA-256, 쓴 파일 목록
<out>/gfx/original/palettes.json         MAIN.EXE 팔레트 9슬롯 + 슬롯별 관찰 메모
<out>/gfx/original/sprites.json          아카이브별 기본 팔레트 슬롯, 항목마다 배치·크기·미디어 키,
                                         항목 묶음(병종·효과; 9.4절)
<out>/gfx/original/hexbchr/000.png ...   미디어 키 original/hexbchr/000 (인덱스 PNG, 색 0 투명)
<out>/gfx/original/sheets/hexbchr.png    아카이브 전체를 한 장에 모은 확인용 시트(16개씩 한 줄,
                                         다른 슬롯을 쓰는 항목은 sheets/<파일>-slot<N>.png)
<out>/gfx/original/facedat/000.png ...   얼굴 240개(64×80, 색 0 불투명), 미디어 키 original/facedat/000
<out>/text/snr0m.json ...                섹션 → 블록(섹션 기준 상대 오프셋, 텍스트)
<out>/maps/battle.json                   전투 맵 목록(이름·크기·칩 세트), 지형 표(코드·이름·전투 장면
                                         배경), 칩별 지형 통계(9.6절)
<out>/maps/battle/000.json ...           전투 맵 하나의 칩 격자(16 px)와 지형 격자(32 px 칸)
<out>/maps/scene.json, campaign.json,    전투 장면 띠, 캠페인 맵(타일·행군로), 도시·궁궐 화면(타일·
<out>/maps/town.json                     보행 격자·표시 지점·물체)
<out>/gfx/original/maps/battle/000.png   전투 맵 그림, battle/chips-1.png·chips-2.png = 칩 뱅크(번호순 16개씩)
<out>/gfx/original/maps/{scene,campaign,town}/...  전투 장면 띠, 캠페인 맵, smap-/pmap- 화면
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
  우선합니다. 원작 레이아웃을 기본 팩 키로 옮기는 매핑은 얼굴 번호 ↔ 무장 대응과 맵 칩 매핑 이후의 과제입니다.
  이 오버레이는 미디어만 바꾸는 임시 경로이고, 장기 목표인 "원작 모드"는 기본 팩을 확장하는 팩으로 계획되어
  있습니다(8절).
* **오버레이가 바꾸지 않는 것**: 오버레이는 이미지·사운드와 `gfx/ui/icons.toml`만 바꿉니다. 전투 화면의 색인
  파일(`gfx/units/units.toml`, `gfx/tiles/terrain.toml`, `gfx/fx/fx.toml`)은 오버레이에 같은 이름으로 두어도
  읽지 않고 팩의 것을 그대로 씁니다. 그래서 프레임·타일 크기가 다른 시트(원작의 48×48/64×64 유닛 스프라이트,
  다른 칩 크기의 타일 아틀라스)를 오버레이에 넣으면 팩의 프레임(24×24)과 `tile_size`(16)로 잘려 그려집니다.
  이런 시트는 오버레이가 아니라 색인 파일과 함께 레이어드 팩(8절)으로 넣어야 합니다.
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
  공개된 얼굴 0번 해시 접두어는 출력 배치가 문서화되지 않아 비교할 수 없어 검사하지 않습니다. 얼굴·`PACKGRP`
  디코딩은 `tests/tfdce_golden.rs`가 따로 확인합니다(240개 64×80·2560바이트·플레인 3 = 0, 38개 크기 목록). 블록 수는 다른 프로젝트의 집계라
  세는 방식이 다를 수 있습니다(불일치 시 메시지에 명시).
  골든 테스트는 주제별로 나뉘어 있어(`golden_korean_ls11_archives`, `_table_containers`, `_map_geometry`,
  `_maps`, `_scenario_text`, `_palette`, `_sprites`, `golden_every_container_validates`, `golden_extraction_succeeds`)
  실패하면 어느 형식이 틀렸는지 이름으로 드러납니다. 검증한 사본에서 손상된 파일은 SHA-256으로 기록해
  (`KNOWN_DAMAGED`) 정확히 기록된 오류로 실패하는지와 손상 전 항목이 복원되는지만 확인합니다.
* **실물 검증 현황 (2026-09, 한국어 DOS/V 사본 1개)**: LS11 코덱·두 디렉터리 바이트 순서, 판본 식별,
  팔레트 뱅크, 스프라이트·칩·전투 UI 배치, 6바이트 테이블(`FACEDAT`/`PACKGRP`)과 TF-DCE 디코딩(얼굴 240개,
  `PACKGRP` 38개)은 실물로 통과했고 PNG를 눈으로 확인했습니다. 메시지 파일 일부(`SNR1M`–`SNR3M`, `IPPAN0M`)는
  실물과 맞지 않아 `golden_korean_scenario_text`와 `golden_extraction_succeeds`(텍스트 종류가 `partial`)가
  실패합니다(텍스트 파서 작업에서 수정 예정).

## 7. 로드맵 (정직한 현황)

| 단계 | 범위 | 현황 |
|---|---|---|
| P0 프로브 | 판본 식별, 공유용 매니페스트 | **완료** (Steam·PC-98은 식별만) |
| P1 컨테이너 | LS11(+인코더), 6바이트 테이블 | **완료**. 음악용 테이블 컨테이너는 P7과 함께 |
| P2 텍스트 | `SNR?M`, `IPPAN0M` (EUC-KR/Big5) | **완료** (화자 번호 분리는 P6 필요). 이름(`BAKDATA`)은 P5 |
| P3 그래픽 | 플레인 셀·팔레트 → PNG | **부분**: 스프라이트·칩·배경 셀·전투 UI 아이콘 완료(실물로 확인), 얼굴(TF-DCE) 완료. `PACKGRP` 화면은 디코딩만 되고 추출 종류는 아직 없음. 오프닝/엔딩(`NPK016`)·`MARK`·`SSCCHR`는 미지원 |
| P4 맵 | `HEXZMAP` 59개, `HEXBMAP`, `MMAP`, `SMAP`/`PMAP` → 타일 맵 JSON + 참고 PNG | **완료**(실물로 확인, 9.6절). 남은 것: 맵별 팔레트 슬롯(P6 시나리오 레코드), `SMAP`/`PMAP` 물체 id의 의미, 지형 코드 255 한 칸 |
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
  정해지지 않았고, 추측하지 않습니다.** 얼굴 번호 ↔ 무장 대응, 맵 칩 대응(P4), 규칙·무장 표(P5), 시나리오(P6)가 규명되는
  순서대로 팩에 들어갈 항목이 늘어납니다.
* **제약**: `extends`는 상대 경로만 허용하므로(웹 빌드와 폴더 이동을 위해) 원작 모드 팩은 기본 팩과 같은 드라이브,
  예컨대 `data/original/`에 둡니다. 브라우저는 로컬 폴더를 읽을 수 없으므로 웹 빌드에는 원작 모드가 없습니다.
  세이브는 최상위 팩의 `id`를 기억하므로 원작 모드의 세이브는 기본 팩의 세이브와 섞이지 않습니다.

## 9. 검증된 형식 사실 (한국어 DOS/V, 실물 1개로 확인)

아래는 사용자가 보유한 한국어 DOS/V `GAME` 폴더(69개 파일)를 읽기 전용으로 분석하고, 디코딩 결과를 PNG로
그려 눈으로 확인한 사실입니다. 조사 노트와 다른 점은 **정정**으로 표시했습니다. 원작 바이트는 저장소에
넣지 않았고, 수치·구조만 적습니다.

### 9.1 판본

* `DISK1.R3I`–`DISK4.R3I`(각 107바이트)는 일본어 DOS/V판 헤더(Shift-JIS `DOS/V 三國志英傑伝 ﾃﾞｨｽｸn
  Ver 1.00 Rel 1.00`, `(C)(P) 1995 KOEI CO.,LTD`, `MADE IN JAPAN`, 끝에 `0x1A`)입니다. **정정**: 노트의
  EUC-KR 헤더 `DOS/V 삼국지영걸전`은 이 사본에 없습니다.
* 대사 파일 7개(`SNR0M`–`SNR4M`, `IPPAN0M`, `BAKDATA`)의 2바이트 쌍 137,114개 중 134,374개(98 %)가
  EUC-KR 한글 영역(선행 0xB0–0xC8, 후행 0xA1–0xFE)에 있고, Big5 전용 후행 바이트는 9개뿐입니다.
* `HEXGRP.R3` 1–2번 항목과 전투 UI 아이콘(`기능`, `아군`, `적군`)도 한국어입니다.

### 9.2 컨테이너

* **LS11 바이트 순서** (**정정**): 매직이 `LS11`이면 디렉터리의 `[저장 길이][복원 길이][오프셋]`이
  빅엔디언, `Ls11`(소문자 s)이면 리틀엔디언입니다. 사전 위치(0x10), 디렉터리 위치(0x110), 4바이트 0 종결,
  체인 규칙, 비트 스트림 코덱은 같습니다. `Ls11`은 `OPGRP.R3`(62개), `END1GRP.R3`(72개), `END2GRP.R3`
  (25개), 나머지 21개는 `LS11`입니다. 역참조 거리는 실물에서 최대 8,188(창 8 KiB), 길이는 최대 512입니다.
* 24개 LS11 아카이브 중 23개는 모든 항목이 선언된 길이로 정확히 복원되고 입력을 전부 소비합니다.
* **`OPGRP.R3`는 검증한 사본이 손상**되어 있습니다: 파일이 디렉터리보다 6바이트 길고(0xB40AA vs
  0xB40B0), 0–26번 항목은 정확히 복원되지만 27번 항목의 그림은 앞부분 약 110줄만 정상이고 그 뒤가
  깨지며, 28번 이후의 압축 항목은 모두 처음 몇 바이트에서 실패하고 원시(raw) 항목도 `NPK016` 헤더 대신
  잡음입니다. 파일 오프셋 약 0x60400(1 KiB 경계)부터 바이트 성격이 바뀝니다(12바이트 반복 문자열이
  압축 데이터 구간에서는 64 KiB당 0–117개 → 이 구간부터 2만 개 이상). 1비트 반전·바이트 삭제로는 복구되지 않았습니다. 디코더 문제가 아니라
  사본의 손상으로 판단하며, 다른 사본으로 교차 확인은 하지 못했습니다.
* **6바이트 테이블** (**정정**): `FACEDAT.R3`/`PACKGRP.R3`의 `u32le 오프셋`은 파일 시작이 아니라 **데이터
  영역(테이블 끝) 기준**입니다(항목 0 = 오프셋 0, 예: `FACEDAT` 항목 1 = 0x516). `table6` 모듈은 이 규칙으로
  읽으며, 항목 수를 구하는 방법과 TF-DCE 압축은 3.1절에 있습니다.

### 9.3 팔레트

* `MAIN.EXE`: MZ 헤더 0x6A00바이트(재배치 6,776개), DGROUP 세그먼트 0x313A. 뱅크는 파일 0x38DF0
  = 데이터 세그먼트 0x1050, 9 슬롯 × 48바이트, 뒤에 `80 40 20 10 08 04 02 01`. 슬롯 5와 8은 같습니다.
* 색 순서 `[B][R][G]` 확인 근거: 슬롯 4는 모든 채널이 0 또는 15인 8색 디지털 팔레트를 두 번 반복하며
  1 = 파랑, 2 = 빨강, 4 = 초록(PC-98 순서)이고, 이 순서로 그린 유닛의 피부·머리·칼날 색이 자연스럽습니다
  (`[G][R][B]`, `[R][G][B]`, `[B][G][R]`는 얼굴이 초록·보라로 나옴).
* 색 0–7은 슬롯 0–3, 5–8에서 같고(윤곽선·UI·유닛 색), 8–15가 슬롯마다 달라 지형 색을 바꿉니다.
* 팔레트 설정 함수(이미지 오프셋 0x118CE)는 슬롯 `n`을 `0x1050 + 48·n`에서 복사합니다. 호출부는 상수
  (슬롯 4)이거나 게임 상태 구조체(DS:0x7A6C)의 +5 바이트 하위 4비트이고, 그 바이트는 시나리오·맵 데이터
  레코드(+8 바이트)에서 읽힙니다. 따라서 **어느 화면이 어느 슬롯을 쓰는지는 데이터가 정합니다.**
  어느 레코드인지는 시나리오(P6) 해석에서 확인할 일입니다.
* 눈으로 본 슬롯 성격: 0 = 도시(`SMAPBGPL` 0번에 맞음), 1 = 초록 들판(전투 배경·칩·캠페인 맵에 맞음),
  2 = 건조·가을 갈색(`SMAPBGPL` 1번 궁궐 실내에 맞음), 3 = 밝은 초록, 4 = 디지털 8색, 5–8 = 갈색 계열.
  추출기의 기본 슬롯: `SMAPBGPL` 0번 → 0, 1번 → 2, 나머지 → 1.
* `OPEN.EXE`(0xC0C6부터 28×48 + 2바이트)와 `END.EXE`(0xD656부터 43×48 + 2바이트; **정정**: 노트는
  16×48)는 아카이브 이름 문자열 바로 뒤에 4비트 값 블록이 있습니다. 슬롯 경계와 어느 그림에 쓰이는지는
  오프닝 그림을 해독하기 전에는 확인할 수 없어 추출하지 않습니다.

### 9.4 스프라이트·칩 배치

셀 = 16×16, 4 플레인 × 32바이트(플레인마다 16줄 × 2바이트, MSB = 왼쪽), 플레인 p = 색 비트 p, 여러 셀은
**행 우선**. 한 방향만 저장됩니다.

| 아카이브 | 항목 | 크기 → 그림 | 내용 (눈으로 확인) |
|---|---|---|---|
| `HEXBCHR.R3` | 181 | 0–168: 2048 B = 4×4 셀(64×64), 169–180: 1152 B = 3×3 셀(48×48) | 전투 장면 유닛 프레임. 병종 순서대로 19묶음: 0–7 단병, 8–15 장병, 16–23 전차, 24–29 궁병, 30–35 연노병, 36–45 투석차(기계·조작병), 46–55 경기병, 56–65 중기병, 66–75 근위대(백마), 76–83 산적, 84–91 악적, 92–99 의적, 100–108 군악대, 109–114 맹수군단, 115–122 무술가, 123–133 요술사, 134–141 이민족, 142–144 민중, 145–153 운송대. 이어서 154–159 불(128×64 세 장을 좌우 반쪽으로), 160–165 물결(같은 방식), 166–168 바위, 169–172 화살·돌, 173–174 음표, 175–177 호랑이, 178–180 수레 |
| `HEXICHR.R3` | 78 | 4608 B = 6×6 셀(96×96) | 기마 무장: 15프레임(달리기·공격 12, 낙마, 쓰러짐, 빈 말) × 5세트 + 75–77 기마 궁수. 세트가 어느 무장인지는 미상 |
| `HEXZCHR.R3` | 47 | 1024 B = 2×4 셀 = 32×32 프레임 2장(위·아래) | 전투 맵 유닛 아이콘. 0–37 = 19병종 × 두 색(주황/초록 계열), 38–39 깃발 보병, 40 백마 전차, 41 불, 42 물, 43–44 책략 효과, 45–46 적토마·황마의 깃발 기병(여포·조조 전용 스프라이트로 추정) |
| `HEXZCHP.R3` | 3 | 80 / 174 / 175 셀 | 전투 맵 칩 (0번 작은 세트, 1번 초원·마을, 2번 산악) |
| `HEXBCHP.R3` | 1 | 224 셀 | 전투 장면 배경(하늘·산·땅) 셀 |
| `MMAPBGPL.R3` | 1 | 255 셀 | 캠페인 맵 셀(강·성·숲·산) |
| `SMAPBGPL.R3` | 2 | 212 / 242 셀 | 0번 도시 야외(아이소메트릭풍), 1번 궁궐 실내 |
| `HEXGRP.R3` | 3 | 0번 14,592 B = **packed planar**, 32 px × 456줄 | 0번: 전투 UI 버튼(`기능`/`아군`/`적군`), 날씨·불·물·바위·책략 아이콘. 1–2번은 그림이 아니라 EUC-KR 대사(무장 퇴각 대사) |

**packed planar**: 8픽셀마다 4바이트(플레인 0, 1, 2, 3)가 이어지고, 그 묶음이 왼쪽에서 오른쪽, 줄은
위에서 아래. `HEXGRP` 0번과 `OPGRP`/`END*GRP`의 원시 그림이 이 형식입니다.

### 9.5 아직 해독하지 못한 것 (추측하지 않음)

* **오프닝·엔딩 (`OPGRP`/`END1GRP`/`END2GRP`)**: 전체 화면 한 장씩이 아닙니다. 항목은 (1) `NPK016`
  헤더 그림 — `"NPK016"`, u16 4, u16 640, u16 400, u16 너비, u16 높이, u16 0, 16 × u16 팔레트
  (`0x0GRB` 12비트), 그 뒤 압축 데이터(코덱 미해독; `TFDED.COM`의 TF-DCE인지도 미확인), (2) 크기 정보가
  없는 packed planar 그림(예: `OPGRP` 21번 30,720 B = 240×256, 26번 57,600 B = 360×320 — 행 간격 추정으로
  그려 보면 인물화가 나옴), (3) 1비트 마스크(`END1GRP`의 같은 크기 항목 쌍)로 섞여 있습니다. 크기는
  `OPEN.EXE`/`END.EXE` 코드에 있을 것이므로 그 해석 전에는 추출하지 않습니다.
* **`MARK.R3`**(1개, 14,584 B)와 **`SSCCHR2.R3`**(29개 × 2,560 B), **`SSCCHR1.R3`**(12개, 크기 제각각,
  첫 바이트들이 개수·번호 목록처럼 보임): 표준 셀·packed·plane 순차 배치 어느 것으로도 그림이 되지
  않았습니다. `SSCCHR2`는 16×16 조각 20개 묶음, `SSCCHR1`은 그 조각의 배치표일 가능성이 있지만 미확인입니다.

### 9.6 맵 (전투·전투 장면·캠페인·도시)

전부 16×16 플레인 셀(9.4절)을 번호로 가리키는 격자입니다. **정정**: 조사 노트의 "10비트 타일 번호"와
"175 이상은 오버레이" 가설은 틀렸습니다.

* **전투 맵 `HEXZMAP.R3` 0–57번**: `[u8 W][u8 H][W×H 칩 바이트][(W/2)×(H/2) 지형 바이트]`
  (그래서 길이가 W×H×5/4 + 2). 칩은 16 px, 지형은 2×2 칩 = 32 px 칸(유닛이 움직이는 격자) 하나에 1바이트.
  W 32–80, H 22–48, 모두 짝수.
* **칩 뱅크**: `MAIN.EXE`가 `HEXZCHP` 0번(80셀)을 버퍼 앞에, 1번(174셀) 또는 2번(175셀)을 그 뒤(+0x2800 =
  80 × 128바이트)에 읽습니다. 칩 바이트는 이 뱅크의 번호 그대로입니다(0–79 공통, 80– 두 번째 세트).
  2번을 쓰는 맵은 `MAIN.EXE`의 u16 목록 19개(0, 1, 4, 8, 10, 16, 20, 28, 29, 30, 35, 36, 39, 40, 42, 43, 44,
  50, 52)이고 나머지는 1번입니다. 이 목록은 맵 번호를 목록과 비교하는 루프
  (`39 87 <목록> 74 0B FE 46 FF 80 7E FF <개수>`)로 찾습니다. 칩 254를 쓰는 맵 0·52가 목록에 있고(1번
  세트로는 253까지뿐), 58개 모두 이 규칙으로 뱅크 안에 들어가며, 그린 결과가 이음매 없이 맞습니다.
* **이름**: `HEXZMAP` 58번(390바이트)은 그림이 아니라 EUC-KR 맵 이름 목록입니다. 줄은 LF로 나뉘고(대부분
  CR LF, 0·1번 사이만 LF), 빈 줄과 `0x1A`로 끝납니다. 게임은 맵 번호만큼 LF를 건너뛴 뒤 선행 바이트
  0xA0 이상인 2바이트 문자만 복사하므로 `신야1`처럼 붙은 숫자는 화면에 나오지 않습니다("… 의 전투").
* **지형 코드** (`MAIN.EXE` 이름 표 20개, 코드 순서 = 표 순서, 칸 그림으로 교차 확인):
  0 평지, 1 숲, 2 산지(녹색 언덕), 3 개울(강·호수 물), 4 다리, 5 성벽, 6 성(성 안 바닥), 7 초원, 8 마을,
  9 낭떠러지(갈색 바위산), 10 문, 11 황무지, 12 울타리, 13 성채(깃발 건물), 14 병영(주황 천막),
  15 군량고, 16 보물창고, 17 집, 18 화염, 19 탁류. 18·19는 정적 맵에 없고(화계·수계로 생기는 상태로
  보임), 맵 32번의 한 칸에 코드 255가 있습니다(의미 미상, 그대로 보존).
* **전투 장면 배경 `HEXBMAP.R3`**: 머리말 없이 `HEXBCHP` 0번(224셀)을 가리키는 격자. 0–4번 230바이트 =
  46×5셀 하늘·지평선(0 바위산, 1 물가 평원, 2 성벽과 성문, 3 숲, 4 평원), 5–8번 528바이트 = 66×8셀 바닥
  (5 돌 포장, 6 다리 널판과 난간, 7 풀밭, 8 흙과 돌). 지형 코드 → 배경·바닥 번호는 `MAIN.EXE`의 20바이트
  표 두 개(`8A 5C 0C 2A FF 8A 87 <표>`가 두 번; 칸 구조체 +0x0C = 지형)입니다: 예) 숲 → 3/7, 다리 → 1/6,
  성 → 2/5, 황무지 → 0/8.
* **캠페인 맵 `MMAP.R3`**: `[W×H 타일][(W/2)×(H/2) 비트]`, 타일은 `MMAPBGPL` 0번(255셀) 번호. 크기는
  `MAIN.EXE`의 장(章)별 `(W, H)` u8 쌍 5개(서장 96×96, 1장 96×96, 2장 72×112, 3장 120×88, 4장 112×128;
  `8A 5E FC 2A FF 8A 80 <표>`로 찾음)이고 4개 항목은 길이가 맞는 크기가 하나뿐입니다. 비트는 32 px 칸마다
  1비트, MSB 먼저, **줄 사이 채움 없이 이어짐**(72폭은 줄당 36비트), **0 = 행군로**입니다. 행군로를 그림
  위에 겹치면 성·전투 표시를 잇는 밝은 길을 정확히 따라갑니다.
* **도시 `SMAP.R3`(12) / 궁궐 `PMAP.R3`(23)**: `[32×20 타일][31×20 보행 격자][u8 n][n × (id, x, y)]`.
  타일은 도시가 `SMAPBGPL` 0번(212셀, 최대 번호 211), 궁궐이 1번(242셀, 최대 241). 보행 격자 점 (x, y)는
  픽셀 (16x + 16, 16y + 8), 즉 타일 x와 x+1 사이 이음매에 있고 0xFF = 막힘, 0x7F = 통행, 그 밖의 값 =
  통행 가능한 표시 지점(궁궐 문·성문·집 앞 등에 놓임). 통행 점을 겹치면 도시의 밝은 길과 정확히 겹칩니다.
  물체 `(id, x, y)`는 x < 31, y < 20이지만 id의 뜻(인물·장식)은 **미확인**입니다.
* **데이터 세그먼트**: 위 표 주소는 DS 기준이며, 파일 위치 = MZ 헤더 크기 + DGROUP × 16입니다. DGROUP은
  진입점의 C 런타임 시작 코드 `mov di, DGROUP`(`B4 30 CD 21 3C 02 73 05 33 C0 06 50 CB BF <DGROUP>`)에서
  읽습니다(한국어판 0x313A → 0x37DA0). 추출기는 이 코드들로 표를 찾으므로 원작 표를 저장소에 담지 않습니다.
* **팔레트**: 게임이 실행 중에 고르므로(9.3절) 추출기는 전투 맵·전투 장면·캠페인 맵에 슬롯 1, 도시에
  0, 궁궐에 2를 씁니다(모두 눈으로 자연스러움을 확인; 맵별 실제 슬롯은 시나리오 해석 후 확인).
* **칩별 지형**: `maps/battle.json`의 `chip_terrain`은 칩마다 그 칩이 그려진 칸의 지형 코드를 전 맵에서 센
  통계와 최다 지형·비율입니다(429칩 중 27개 미사용, 118개는 최다 비율 80 % 미만 — 경계 칩). 칸의 지형은
  맵의 지형 격자가 정하므로, 변환기는 지형 격자를 쓰고 칩 통계는 참고로만 씁니다.

---

## English summary

The **original-data importer** is an optional, experimental feature for players who **own** a copy of KOEI's
1995 *Sangokushi Eiketsuden*. The game never needs it. It reads the player's install **read-only**, uploads
nothing, never circumvents copy protection (an encrypted container stops at detection), and writes neutral files
(PNG, UTF-8 JSON) into a local folder the player chooses (never inside the install; `data/original/` is
git-ignored). It is a clean-room implementation from published format facts only; no third-party code was used,
and the repository and CI contain no original bytes (tests use synthetic fixtures from our own encoders).

* **Verified on a real Korean DOS/V copy (section 9)**: the copy keeps the Japanese DOS/V disk header
  (Shift-JIS) and has EUC-KR Hangul text, so identification requires both. `LS11` archives have a big-endian
  directory, `Ls11` ones (`OPGRP`, `END1GRP`, `END2GRP`) a little-endian one; 23 of 24 archives decode
  exactly, and the owner's `OPGRP.R3` is damaged from about offset 0x60400 (recorded by hash in the golden
  tests). The palette bank sits at 0x38DF0, `[B][R][G]` confirmed; the game picks the slot at run time from
  data, so the extractor uses a visually checked slot per archive. Sprite/chip geometries and entry groups
  (the 19 unit classes in class order, effects, map icons, mounted officers) are listed in section 9.4 and
  written to `gfx/original/sprites.json` with contact sheets in `gfx/original/sheets/`. `HEXGRP.R3` entry 0
  uses a *packed* planar layout (4 plane bytes per 8 pixels). Not decoded: `NPK016` opening/ending pictures,
  `MARK.R3`, `SSCCHR1/2.R3`. The 6-byte tables use offsets relative to the data area (section 3.1), and
  some message files do not parse yet.
* **Editions**: Korean DOS/V (identified by the `DISK1.R3I` header and Korean text, extractable), Traditional-Chinese DOS
  (DOS/V family + Big5 text, medium confidence, extractable but not yet verified on a real copy), Steam 2017 and
  PC-98 disk images (identified only), anything else `unknown` (refused unless `--edition` is given).
* **Assets**: LS11 archives and 6-byte tables with full invariant checks; message text (EUC-KR/Big5 → JSON,
  scene counts cross-checked); palettes located by signature in `MAIN.EXE`; 16×16 planar sprite/chip cells and packed
  planar images → PNG (plane order, cell order and palette slots checked visually on the real files); the 240
  **TF-DCE portraits** of `FACEDAT.R3` → `gfx/original/facedat/<nnn>.png` (64×80, decoder written from a static
  reading of `TFDED.COM`, format in section 3.1; all 240 consume their input exactly and were checked by eye).
  `PACKGRP.R3` (38 TF-DCE screens and event illustrations, not 16×16 tiles) decodes but is not an extraction kind
  yet. **`BAKDATA.R3` names are reported as unsupported**, because the record layout is unknown.
* **Usage**: `hero-tools original probe <dir> [--out manifest.json]`, then
  `hero-tools original extract <dir> --out <overlay> [--text] [--sprites] [--portraits]`, then
  `eiketsuden --original <overlay>` (or `EIKETSUDEN_ORIGINAL`; native builds only). Media keys are looked up in
  the overlay first, then in the pack. The extracted keys live under `original/...` and do not replace the base
  pack's own keys automatically yet. The overlay replaces images, sounds and `gfx/ui/icons.toml` only: the
  battle index files (`units.toml`, `terrain.toml`, `fx.toml`) always come from the pack, so sheets with other
  frame or tile sizes belong in a layered pack (section 8), not in the overlay.
* **Help wanted**: run `probe` on a Steam install (`steamapps/common/Eiketsuden1`) and attach the manifest to an
  issue. A manifest contains relative paths, sizes, SHA-256, the first 16 bytes of each file and container
  summaries — no game content, no absolute paths.
* **Verification**: `EIKETSU_ORIGINAL_DIR=<data folder> cargo test -p hero-import --test golden` checks the
  published known answers on a real install. The golden tests are split by topic; on the verified copy the LS11, edition, 6-byte-table, palette, sprite,
  map-geometry and map (`golden_korean_maps`) checks pass (as do the TF-DCE checks in `tests/tfdce_golden.rs`); the message-file checks, and
  with them the full-extraction check, do not yet.
* **Maps (section 9.6, verified on the real copy)**: a battle map of `HEXZMAP.R3` is `[W][H][W×H chip bytes]
  [(W/2)×(H/2) terrain bytes]` (hence the ×5/4 length; not 10-bit indices, and values ≥ 175 are ordinary chips,
  not overlays). The chip byte indexes a bank of `HEXZCHP` entry 0 (80 cells) followed by entry 1 or 2; the
  19 maps that use entry 2 are a list in `MAIN.EXE`. Entry 58 is the LF-separated EUC-KR name list. The 20
  terrain codes (plain, forest, hill, stream, bridge, wall, castle, grassland, village, cliff, gate, wasteland,
  fence, fortress, barracks, granary, treasury, house, fire, flood) and the terrain → battle-scene strip tables
  (`HEXBMAP`: five 46×5 backdrops, four 66×8 grounds over `HEXBCHP`) come from `MAIN.EXE`. Campaign maps
  (`MMAP`) are `[W×H tiles][route bits, 0 = road, rows packed]` with sizes from a per-chapter table; town and
  palace screens (`SMAP`/`PMAP`) are 32×20 tiles, a 31×20 walk grid (0xFF blocked, 0x7F open, other = marked
  point) and `(id, x, y)` objects of unknown meaning. All tables are located through the code that reads them
  (`--maps` writes `maps/*.json` and `gfx/original/maps/`).
* **Roadmap**: P4 maps done (per-map palette slot and the town object ids open); P5 rules/officer tables, P6
  scenario bytecode, P7 OPL2 music, P8 Steam/PC-98 — open.
* **Original mode (planned, not implemented)**: the goal is a pack the importer writes to `data/original/`
  (git-ignored) with `extends = "../base"` and `[presentation] canvas = [640, 480]`, holding only what was
  converted from the player's copy; everything else keeps coming from the base pack through the layered-pack
  chain (`extends`, which exists: see MODDING.md "Layered packs"), so the mode can grow asset by asset. The
  importer does not write such a pack yet, and no mapping from original files to the pack's keys has been
  decided. `extends` is relative only, so the pack lives next to the base pack; there is no original mode on
  the web.
