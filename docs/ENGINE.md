# 엔진은 어떻게 만들어졌나

> **English summary** — How Eiketsuden Reloaded is built. It follows the OpenRCT2/OpenTTD model (an
> open-source engine; the content is data) but not their route of reverse-engineering the original program's code
> into a re-implementation: the engine is written from scratch from the published rules, content ships as
> license-clean data packs, and the original game is only read — from the player's own copy, never run — and
> converted into the same pack format at every launch (native builds; the development tools can also write the
> result to a local folder the player chooses). The page
> explains the three layers (engine, base pack, original mode), how the rules and the original formats were
> established, how a converted battle reaches the screen, and the development practice (data validation,
> deterministic AI-vs-AI simulation, synthetic fixtures plus gated golden tests on a real copy, recorded
> decisions, reviewed pull requests).

이 문서는 영걸전 Reloaded가 **무엇을, 어떤 방식으로** 만들고 있는지 설명합니다. 코드 구조의 세부는
[ARCHITECTURE.md](ARCHITECTURE.md), 설계 결정의 기록은 [DECISIONS.md](DECISIONS.md), 원작 파일 형식은
[reverse-engineering/](reverse-engineering/README.md)에 있습니다.

## 1. 한 줄 요약

**원작 게임을 흉내 내는 새 엔진 + 그 엔진이 읽는 데이터.** [OpenRCT2](https://openrct2.io/)가 롤러코스터 타이쿤 2를,
[OpenTTD](https://www.openttd.org/)가 트랜스포트 타이쿤을 되살린 것과 같은 모델입니다. 엔진은 오픈소스
(GPL-3.0-or-later)이고,
화면에 나오는 모든 것(규칙 수치·무장·맵·전투·대사·그림·음악)은 **데이터 팩**에서 옵니다.

## 2. OpenRCT2와 같은 점, 다른 점

| | OpenRCT2 | 영걸전 Reloaded |
|---|---|---|
| 목표 | 원작과 같은 게임을 현대 환경에서 | 같음(PC판 규칙, Windows·macOS·Linux·웹) |
| 출발점 | 원작 실행 파일을 **디컴파일**해 함수 단위로 C/C++로 옮기며 원작 바이너리를 점점 대체(OpenTTD도 원작을 역어셈블한 재구현에서 출발) | 원작 코드를 옮기지 않고, 공개된 규칙 공식과 플레이 사실로 **엔진을 처음부터 새로 작성**(Rust) |
| 원작 파일 | 설치본이 **필수**(그래픽·사운드·시나리오를 읽음) | **선택**. 원작 없이도 끝까지 플레이할 수 있는 라이선스 청정 기본 팩을 함께 배포 |
| 원작 데이터 사용 | 실행할 때 원작 형식을 직접 읽음 | 실행할 때마다 원작 파일을 **엔진의 팩 형식으로 변환**해 메모리에 얹음(원작 모드, 네이티브 빌드 전용. 웹에서는 로컬 폴더를 읽을 수 없음) |
| 원작 실행 파일 | 코드의 원천 | 데이터 표(팔레트·맵 표·칸 변경 표)를 찾는 대상일 뿐. **읽기 전용 정적 분석**, 실행하지 않음 |

이렇게 한 이유는 [DECISIONS.md](DECISIONS.md) D3(에셋 정책)·D5(공개)·D6(클린룸 임포터)에 있습니다. 요약하면
(1) 저장소와 배포물에 KOEI의 그래픽·음악·텍스트·실행 코드를 넣지 않고, (2) 원작 없이도 누구나 플레이·모딩할 수 있게
하기 위해서입니다. 예외는 원작 형식을 설명하는 데 필요한 사실입니다. 형식 문서의 구조와 몇몇 표 값, 그리고 사용자
실행 파일에서 표를 찾기 위한 짧은 코드 서명이 여기에 해당합니다([FORMATS §15](reverse-engineering/FORMATS.md#main-exe)).

## 3. 세 층

```text
┌──────────── 엔진 (crates/hero-core, hero-game) ────────────┐
│ 규칙 공식, 전투·AI, 캠페인, 대사 스크립트, 세이브, 화면     │  ← 원작 콘텐츠 없음
└──────────────▲──────────────────────────▲──────────────────┘
               │ 같은 팩 형식             │
┌──────────────┴───────────┐  ┌───────────┴──────────────────────────────┐
│ 기본 팩 data/base        │  │ 원작 모드 팩 (사용자 PC에서만 만들어짐)   │
│ 공개 라이선스 그림·음악  │◀─│ extends = 기본 팩. 원작 설치본에서 변환한 │
│ 새로 쓴 대사 (CREDITS)   │  │ 얼굴·유닛·지형·맵·전투·전투 대사          │
└──────────────────────────┘  └──────────────────────────────────────────┘
```

1. **엔진** — 규칙과 동작만 가집니다. 전투는 결정적(시드 난수)이고 그래픽·파일 입출력과 분리되어 있어, 같은 코드가
   게임·명령줄 도구·테스트에서 그대로 돕니다.
2. **기본 팩** — 원작의 PC판 규칙 수치와 『삼국지연의』(퍼블릭 도메인)를 바탕으로 새로 쓴 서장·제1장 캠페인, 라이선스가
   분명한 그림·음악. 원작이 없는 사람도 이것으로 플레이합니다.
3. **원작 모드** — 정품을 가진 사용자가 타이틀의 "원작 데이터"에서 설치 폴더를 고르면, 게임이 **실행할 때마다**
   원작 파일을 읽어 기본 팩을 확장하는 팩(`extends`)을 메모리에서 만들고 그 위에서 플레이합니다(D8·D10). 변환된 것은
   원작 것으로, 아직 변환하지 못한 것은 기본 팩 것으로 보입니다. 그래서 원작 모드는 변환기가 늘어날수록 한 종류씩
   원작의 모습에 가까워집니다.

## 4. 규칙은 어떻게 재현했나

* **출처**: PC판(국내 DOS판)을 팬 커뮤니티가 역분석해 공개한 공식(공격·방어·데미지·반격·사기·책략·날씨 등)을 사실
  정보로 옮겼습니다. 공식과 출처, 우리가 정한 부분은 [RULES.md](RULES.md)에 항목마다 적었습니다.
* **데이터로 분리**: 병종 능력치·지형 비용·책략 범위·아이템 효과는 코드가 아니라 `rules/*.toml`에 있습니다. 모드는
  이 파일만 바꿔 규칙을 조정할 수 있고, 원작 모드는 나중에 원작 실행 파일의 규칙 표로 이 파일을 만들 수 있습니다
  (STATUS 4절 3단계).
* **검증**: `hero-tools simulate`가 모든 전투를 AI 대 AI로 여러 시드로 돌려 승률·턴 수를 보고합니다. 규칙이나 전투
  데이터를 바꾸면 이 결과로 밸런스가 무너지지 않았는지 확인합니다.

## 5. 원작 파일은 어떻게 읽나 (클린룸)

원작 형식은 보유한 한국어 DOS/V판 한 벌을 **읽기 전용·정적 분석**으로 조사해 정했습니다([METHOD.md](reverse-engineering/METHOD.md)).

* **구조 불변식을 정답 삼기**: 컨테이너 디렉터리가 파일 끝에서 정확히 끝나는지, 압축이 선언한 길이로 풀리는지,
  대사 파일의 모든 바이트가 시나리오 스크립트로 덮이는지 같은 조건을 실물 전체에 걸어 가설을 검증합니다.
* **실행 파일은 표를 찾는 대상**: 맵 칩 뱅크 목록·성문/적교 변경 표 같은 데이터는 그 표를 **읽는 코드의 바이트
  패턴**(코드 서명)으로, 팔레트는 표 자체의 모양(데이터 서명)으로 찾습니다. 게임은 표 값을 저장소가 아니라 사용자의
  실행 파일에서 읽습니다. 다른 빌드에도 통하도록 설계했지만 확인한 것은 한국어판 한 벌뿐입니다
  ([FORMATS §15](reverse-engineering/FORMATS.md#main-exe)).
* **뜻을 모르면 추측하지 않음**: 형식 문서의 모든 항목에 [검증]·[코드]·[추론]·[미상]을 표시합니다. 추론으로 옮긴 것
  (예: 화면별 팔레트 슬롯)은 변환 결과와 문서에 그렇다고 적고, 코드로 확인되면 표기를 바꿉니다.
* **저장소에는 사실만**: 원작의 그림·대사·음악과 파일 자체는 저장소와 CI에 없습니다(형식 사실과 코드 서명은 위의
  예외). 테스트는 우리 인코더로 만든 합성 데이터를 쓰고, 실물 검증은 원작을 가진 개발자의 PC에서만 도는 골든
  테스트(`EIKETSU_ORIGINAL_DIR`)로 합니다.

## 6. 원작 전투가 화면에 오기까지 (예: 하비 전투)

1. **프로브·판본 식별** — 설치 폴더의 파일 구성과 텍스트 통계로 판본을 정합니다(한국어 DOS/V, 번체 중문 DOS 등).
2. **해독** — `HEXZMAP`(맵 칩·지형), `HEXZCHR`(유닛 아이콘), `SNR1D`/`SNR1M`(시나리오 바이트코드와 대사), `BAKDATA`
   (무장·아이템), `MAIN.EXE`(팔레트·표)를 읽습니다.
3. **팩 키로 옮기기** — 원작 맵은 그림 층 + 규칙 층의 맵 파일로(D9), 원작 전투는 기본 팩의 같은 전투를 원작의 맵·배치·
   명단·보물·증원으로 다시 짠 전투 파일로(D11) 만듭니다. 전투 블록의 트리거 레코드는 엔진 이벤트가 됩니다(D12):
   예컨대 하비에서 "30턴이 되거나, 유비가 후성·위속·송겸과의 일기토(플래그 17·18·19)를 모두 마친 뒤 (12, 12) 칸에
   서면 적교가 내려오고 조조군이 합류한다"는 원작 스크립트(FORMATS §13.2)는 `turn_start`·`reach` 트리거, `when`
   플래그 조건, `set_terrain`(바뀐 칩으로 그린 칸 그림과 함께)·`spawn`·`set_ai`·`set_stage` 동작과 원작 대사로 만든
   대사 장면이 됩니다.
4. **검증** — 개발 중에는 `hero-tools original pack`과 골든 테스트로 변환 결과가 기본 팩과 같은 검증기를 통과하는지
   확인합니다. 게임은 실행할 때 검증하지 않고, 팩을 불러오지 못하면 오류 화면으로 알립니다.
5. **플레이** — 엔진은 이 팩을 기본 팩과 똑같이 읽습니다. 원작 바이트코드를 해석하는 별도 실행 경로는 없습니다.

## 7. 개발 방식

* **데이터 먼저, 엔진은 최소로**: 새 기능은 데이터 형식(TOML 필드)과 검증 규칙부터 정하고, 모든 필드는 선택 사항으로
  추가해 기존 팩이 깨지지 않게 합니다. 한 번만 쓰일 추상화는 만들지 않습니다.
* **결정은 기록으로**: 되돌리기 어려운 결정(데이터 형식, 공개 형식, 원작 데이터를 다루는 방식)은 대안과 탈락 이유를
  [DECISIONS.md](DECISIONS.md)에 남깁니다. 당장 하지 않는 개선점은 [BACKLOG.md](../BACKLOG.md)와 이슈로 보냅니다.
* **검증의 층**
  1. 팩 로딩과 교차 참조 검증: 모르는 id·맵 밖 좌표·도달할 수 없는 단계·설정되지 않는 플래그 등([MODDING.md](MODDING.md#validation)).
  2. 단위 테스트: 규칙 공식, 전투 흐름, AI, 스크립트, 원작 형식 디코더(합성 픽스처).
  3. 골든 테스트: 원작 실물에서만 도는 테스트로 형식 사실과 변환 결과를 확인합니다.
  4. 시뮬레이션: 모든 전투를 AI 대 AI로 끝까지 돌려 봅니다.
  5. 실제 게임: 화면에 영향을 주는 변경은 네이티브·웹 빌드를 실제로 띄워 확인합니다.
* **CI**: 모든 PR에서 포맷, clippy(네이티브·wasm), 테스트, 기본 팩 검증, 모든 전투의 AI 대 AI 시뮬레이션, 웹 빌드,
  에셋 파이프라인 검사를 돌리고 Windows·macOS에서도 빌드합니다. main은 GitHub Pages 웹 데모로 배포됩니다.
* **리뷰**: 변경은 PR로 올리고, 자동 코드 리뷰와 사람의 지적을 모두 읽습니다. 결함은 같은 PR에서 고치고, 설계와 맞지
  않는 지적은 근거를 남기고, 범위 밖의 것은 BACKLOG로 넘긴 뒤 머지합니다. 처리 결과는 PR에 기록합니다.

## 8. 참여하기

* **플레이하고 알려 주기**: 버그와 밸런스 의견은 이슈로.
* **모드·콘텐츠**: [MODDING.md](MODDING.md)의 형식으로 전투·장면·규칙을 만들 수 있습니다. 레이어드 팩(`extends`)으로
  바꾸는 파일만 담으면 됩니다.
* **원작 판본 제보**: Steam판·번체 중문판·PC-98판을 가진 분은 `hero-tools original probe`로 만든 매니페스트(파일 이름·
  크기·해시·형식 식별용 첫 16바이트·컨테이너 요약. 복원된 데이터·텍스트·그림은 없음)를 이슈에 올려 주시면 형식
  조사에 큰 도움이 됩니다([ORIGINAL_DATA.md](ORIGINAL_DATA.md) 5절).
* **개발**: 빌드와 테스트는 [DEVELOPING.md](DEVELOPING.md), 코드 구조는 [ARCHITECTURE.md](ARCHITECTURE.md).
