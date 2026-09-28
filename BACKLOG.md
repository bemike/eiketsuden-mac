# Backlog

리뷰 2차(2026-09-26)에서 확인됐지만 이번 범위에서 고치지 않은 비차단 항목입니다. 형식은 **무엇을 · 왜 · 영향 범위**입니다.
이미 처리된 항목은 넣지 않았습니다. 처리된 것은 세이브 슬롯 팩별 분리, 세이브 문서, 레이어드 팩의 웹 배포 안내, JS 번들 버전 CI 검사, wasm clippy CI입니다.

## 원작 모드 착수 전에 할 것

- [ ] **미디어 색인 스키마를 hero-core로 이전**
  - 무엇을: `units.toml`·`terrain.toml`·`fx.toml`의 serde 타입(SpriteDef/FxDef/Tileset)과 `validate_layer`를 hero-game에서 hero-core로 옮긴다. `Pack::missing_media`가 타입 오류를 Error로 보고하게 하고, 색인 파일을 `unknown_fields` 린트에 넣는다.
  - 왜: 지금은 검증기가 키 존재만 봐서 `anchor` 하나가 빠져도 validate는 통과하고, 게임에서는 스프라이트가 조용히 깨진다. 임포터도 같은 스키마를 써야 한다.
  - 영향 범위: hero-core `pack/media.rs`, hero-game `screens/battle/{sprites,tileset}.rs`, `docs/ASSETS.md`

## 원작 모드 팩 (2026-09-26 `original pack` 도입 후)

- [ ] **학습 타일셋의 경계 개선**
  - 무엇을: 초원·산지·황무지처럼 원작이 대각선·부분 칸으로 잇는 지형에 8방향 마스크나 칸 변형(`cells` 여러 개)을 학습시킨다. 원작 맵 자체는 이제 그림 층이 있는 맵 파일로 팩에 들어가므로(D9), 원작 전투가 생기면 이 타일셋은 기본 팩 맵을 그릴 때만 쓰인다.
  - 왜: 4방향 최빈 칸만 써서 기본 팩 맵 위에서는 네모난 가장자리가 보인다.
  - 영향 범위: hero-import `pack.rs`(`learn_tiles`), 엔진 `auto` 층 규칙(8방향이면 ASSETS.md 변경)
- [ ] **무장 전용 유닛 아이콘(`HEXZCHR` 38–40, 45–46)과 상태 아이콘(43–44)**
  - 무엇을: 원작은 유비에게 38 + 병종, 여포·조조에게 45·46, 상태 `& 0x02`인 유닛에게 43·44를 그린다(FORMATS §8.2). 상태 `& 0x02`의 뜻을 확인하고, 무장 전용 아이콘을 원작 모드 유닛 시트로 옮긴다.
  - 왜: 지금은 모든 유닛을 병종 아이콘으로 그린다. 엔진에는 무장별 유닛 시트가 없어 형식 추가가 필요하다.
  - 영향 범위: hero-import `pack.rs`(`convert_units`), 엔진 유닛 시트 선택(무장별 시트), ASSETS.md

## 원작 전투 (2026-09-28 `battles.rs` 도입 후)

- [ ] **원작 전투 이벤트의 남은 부분 (D12 이후)**
  - 무엇을: (1) 공유 플래그 조건이 맞지 않을 때의 분기(하비 적교 칸에서 세 장수를 만나기 전 여포의 대사)를 조건의 부정으로 옮긴다(`when`에 "하나라도 아님"이 필요). (2) 기본 팩 이벤트가 없는 곳의 `set_country`(소속 변경)를 캠페인 합류로 옮긴다. (3) 전투 중 `set_objective`로 바뀌는 목표 문구를 보여 줄 엔진 동작. (4) 원작의 일기토 연출(`duel_action` 동작 번호 해독 후).
  - 왜: 지금은 전투 파일 주석에 "not converted"로 남는다. 하비 분기는 대사 하나, 나머지는 서장·1장에서 기본 팩 이벤트가 대신 맡는다.
  - 영향 범위: hero-import `battles.rs`, hero-core `battledef.rs`(조건 형식), hero-game 전투 HUD
- [ ] **시뮬레이터 플레이어 AI가 사수관(원작 맵)에서 마을로 물러나 멈춤**
  - 무엇을: `hero-tools simulate`로 원작 모드 사수관을 돌리면 아군이 8턴 이후 마을 칸에서 움직이지 않아 턴 제한 패배한다. AI의 회복·목표 선택을 확인한다.
  - 왜: 원작 모드 전투의 밸런스 확인에 시뮬레이터를 쓸 수 없다(데이터는 정상, 실제 플레이는 가능).
  - 영향 범위: hero-core `battle/ai.rs`
- [ ] **`advance`도 목표 칸 옆에서 멈출 수 있음**
  - 무엇을: `approach`의 거리 표는 목표 칸 진입 비용을 0으로 쳐서, `advance` 유닛이 목표 칸 옆에 선 채로 대기로 넘어가지 못할 수 있다. `march`는 PR #13에서 목표 칸이 이동 범위 안이면 그 칸을 고르도록 고쳤다.
  - 왜: 원작 방식 4(목표 칸으로 이동)와 짝을 이루는 칸 트리거(종류 6)가 늦게 발동하거나 발동하지 않을 수 있다. 다만 `advance`는 가는 길에 싸우는 방식이라 영향이 작다.
  - 영향 범위: hero-core `battle/ai.rs`(`Advance` 분기)

- [ ] **맵 32 전투를 변환할 때 (5, 13)의 코드 255 칸 확인**
  - 무엇을: 맵 32를 쓰는 전투가 변환 대상이 되면 그 칸 근처의 유닛 등장·배치와 AI 목표 칸을 원작과 비교한다.
  - 왜: 원작은 이동으로는 그 칸에 못 들어가지만 배치 탐색·AI 목표·먼 거리 탐색은 들어갈 수 있는 칸으로 친다(FORMATS §10.4). 변환기는 절벽으로 근사한다.
  - 영향 범위: hero-import `pack.rs`(`OFF_MAP`), 원작 전투 변환

## 견고성 · 회귀 방지

<<<<<<< HEAD
- [ ] **자식 팩의 `credits.txt`가 부모 팩의 저작자 표시를 가림**
  - 무엇을: 레이어드 팩의 제작진 화면이 체인의 모든 `credits.txt`를 이어 보이거나(가까운 팩부터), 자식 팩이 부모 표시를 포함하도록 validate가 경고한다.
  - 왜: 지금은 가장 가까운 팩의 파일 하나만 보여, 부모(기본 팩)의 CC BY·CC BY-SA 저작자 표시가 게임 안에서 사라질 수 있다(원작 모드 팩은 `credits.txt`가 없어 해당 없음).
  - 영향 범위: hero-game `screens/credits.rs`, hero-core `pack/media.rs`, MODDING
- [ ] **`DataRoot::path`를 최상위 팩 전용으로 줄이기**
  - 무엇을: 미디어가 모두 `media_paths`로 옮겨 가 `path()`의 체인 탐색(`memfs::is_file`)은 테스트만 쓴다. 최상위 팩 경로만 돌려주게 하고 테스트·ARCHITECTURE·DECISIONS·memfs 설명을 정리한다.
  - 왜: 쓰이지 않는 경로가 문서에 살아 있는 것처럼 남는다.
  - 영향 범위: hero-game `platform/mod.rs`, `platform/memfs.rs`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`
=======
- [ ] **원작 폴더 탐색기의 막힐 수 있는 I/O를 작업 스레드로**
  - 무엇을: 입력한 경로의 `is_dir`/`read_dir`(응답 없는 UNC 경로는 SMB 시간 초과 동안 멈춤)와 X11 `clipboard_get`(클립보드 소유자가 응답하지 않으면 기다림)을 메인 스레드 밖에서 하거나 시간 제한을 둔다.
  - 왜: 드물지만 그동안 게임 창이 멈춘다. 목록 탐색으로는 UNC에 갈 수 없었고 경로 입력(PR #17)으로 생긴 경로다.
  - 영향 범위: hero-game `original.rs`(`typed_folder`, `list`), `screens/original.rs`
>>>>>>> origin/main
- [ ] **유닛 프레임 크기의 단일 출처화**
  - 무엇을: 캠프·갤러리 화면(`ui/art.rs unit_frame_size`)이 프레임을 시트 크기÷(4×6)로 추정하는 방식을 `units.toml`의 `frame`으로 통일하거나, 시트 크기를 validate에서 대조한다.
  - 왜: 시트에 여백이 있으면 전투와 캠프의 결과가 어긋난다.
  - 영향 범위: hero-game `ui/art.rs`, hero-core media 검증
- [ ] **전투 맵 캐시 텍스처 크기 상한**
  - 무엇을: 맵 캐시 render target 크기에 상한을 두고 validate에서 경고하며, 넘치면 청크로 분할한다.
  - 왜: 큰 맵과 큰 타일이 겹치면 모바일 WebGL 텍스처 한도(2048~4096)를 넘어 맵이 검게 나온다.
  - 영향 범위: hero-game `screens/battle/tileset.rs`, hero-core 검증, `docs/ASSETS.md`
- [ ] **전투의 유닛 참조 열거를 한곳으로 모으기**
  - 무엇을: exhaustive match로 된 `BattleDef::unit_refs()`를 만들어 validate와 simulate의 `player_needs`가 함께 쓰게 한다.
  - 왜: 조건·이벤트 variant가 추가되면 simulate가 조용히 왜곡된 승률을 낸다.
  - 영향 범위: hero-core `battledef.rs`, hero-tools `validate.rs`·`simulate.rs`
- [ ] **중단된 추출의 재시도 가능화**
  - 무엇을: `extract`가 도중에 실패해도 `index.json`을 남기거나, 임시 폴더에 쓴 뒤 교체한다.
  - 왜: 지금은 중단된 출력 폴더를 다음 실행이 `OutputNotEmpty`로 거부한다.
  - 영향 범위: hero-import `extract.rs`
- [ ] **모드 팩 텍스트의 한자 커버리지 경고**
  - 무엇을: 팩 텍스트에 쓰인 한자 중 폰트에 없는 글자를 validate가 경고한다.
  - 왜: 지금은 기본 팩만 CI 폰트 테스트로 확인한다.
  - 영향 범위: hero-core/hero-tools, 폰트 cmap 파서

## 배포 · CI

- [ ] **웹 사이트 조립 로직 단일화**
  - 무엇을: `pages.yml`, `build.sh`, `build.ps1` 세 곳에 중복된 조립 로직을 하나로 모은다. `--data`로 extends 팩을 복사할 때 부모 팩도 형제 디렉터리로 복사하거나 경고한다.
  - 왜: 세 곳이 서로 어긋날 수 있다.
  - 영향 범위: `tools/web`, `.github/workflows/pages.yml`
- [ ] **Pages 배포를 CI 성공에 연동**
  - 무엇을: `workflow_run`으로 CI가 성공했을 때만 Pages를 배포한다. 릴리스 빌드에도 `cargo test -p hero-core`와 validate를 넣는다.
  - 왜: 지금은 CI가 실패해도 배포된다.
  - 영향 범위: `.github/workflows`
- [ ] **JS 플러그인 버전 일치 검사**
  - 무엇을: `hero_web.js`의 `version`과 `HERO_WEB_VERSION`이 같은지 CI에서 검사한다.
  - 왜: 지금은 사람이 기억해서 맞춰야 한다.
  - 영향 범위: `.github/workflows/ci.yml`

## 게임 · 콘텐츠

- [ ] **캠페인을 따라가는 밸런스 시뮬레이션**
  - 무엇을: `hero-tools simulate --campaign`을 추가한다. 선택지를 지정할 수 있고, 레벨과 영입을 이어받는다. 프로토타입은 제1장 작업 때 만든 워커를 참고한다.
  - 왜: 지금 simulate는 새 게임 초기 군대로만 싸워서 제1장 전투가 모두 0%로 나온다.
  - 영향 범위: hero-tools
- [ ] **배치 칸 기준 도달성 경고**
  - 무엇을: defeat_all 전투에서 배치 칸에서 닿을 수 없는 적을 validate가 경고한다.
  - 영향 범위: hero-core `validate.rs`
- [ ] **가장자리 스크롤 개선**
  - 무엇을: 웹·플랫폼 계층에서 mouseleave와 포커스 상실을 전달해 가장자리 스크롤을 정확히 멈춘다.
  - 왜: 지금은 1초 타이머로 근사한다.
  - 영향 범위: `web/hero_web.js`, hero-game `input`
- [ ] **480×270보다 작은 캔버스 대응**
  - 무엇을: 출진 준비와 전투 화면에 작은 캔버스용 레이아웃을 만든다.
  - 왜: 지금은 validate 경고만 한다.
  - 영향 범위: hero-game `screens/camp`, `screens/battle`
- [ ] **웹 첫 로딩 경량화**
  - 무엇을: 폰트(gzip 약 1.6MB)를 서브셋하거나 지연 로딩한다.
  - 영향 범위: `tools/assets`, hero-game `assets`
- [ ] **이후 콘텐츠 제작**
  - 무엇을: 제2장(관도 ~ 장판파) 이후 캠페인과 IF 루트, 영어 번역을 만든다.
  - 영향 범위: `data/base`
