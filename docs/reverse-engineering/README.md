# 원작 데이터 분석 자료 (역공학 문서)

> **English summary** — This folder is the durable record of how the data files of KOEI's 1995 *Sangokushi
> Eiketsuden* (DOS/V family, verified on one owned Korean DOS/V copy) were reverse-engineered for the
> clean-room importer in `crates/hero-import`. [FORMATS.md](FORMATS.md) is the consolidated format
> specification with a confidence mark on every item; [METHOD.md](METHOD.md) is a reusable playbook (order of
> work, techniques, pitfalls, a checklist for other KOEI titles of the era such as Koumeiden or Sousouden);
> [SCENARIO.md](SCENARIO.md) explains how the scenario flows (blocks, choices, questions, jumps, officers joining and
> leaving, battle blocks) chapter by chapter, with a block-level outline in [SCENARIO_FLOW.md](SCENARIO_FLOW.md) (no
> original text); [STATUS.md](STATUS.md) lists what is decoded, partly decoded and open, and the remaining steps to a
> playable original mode. The work followed strict rules: the owner's own copy only, read-only, nothing from
> the install ever executed, static analysis for interoperability only, no KOEI bytes or data tables in the
> repository or CI, and real-data tests gated behind `EIKETSU_ORIGINAL_DIR`. The user guide for the importer
> remains [../ORIGINAL_DATA.md](../ORIGINAL_DATA.md).

이 폴더는 『삼국지 영걸전』(KOEI, 1995) DOS/V판 데이터 파일을 **어떻게 분석했고 무엇을 알아냈는지**를 나중에 다시
쓸 수 있게 정리한 기록입니다. 임포터 사용법은 [../ORIGINAL_DATA.md](../ORIGINAL_DATA.md)에 있고, 여기에는 형식
명세·방법론·현황만 둡니다. 같은 시기 KOEI 게임(공명전·조조전 등)을 분석할 때도 출발점으로 쓸 수 있게 썼습니다.

## 문서 구성

| 문서 | 내용 | 이런 때 읽기 |
|---|---|---|
| [FORMATS.md](FORMATS.md) | 파일·파일군별 형식 명세: 판본 식별, LS11/Ls11, 6바이트 테이블, TF-DCE, 팔레트, 평면 그래픽·스프라이트, 오프닝·엔딩, 맵, 메시지, 시나리오 바이트코드, `IPPAN0`, `BAKDATA`, `MAIN.EXE` 서명. 항목마다 신뢰도 표기 | 파서를 고치거나 새 판본·새 기능을 붙일 때 |
| [METHOD.md](METHOD.md) | 작업 순서, 기법(구조 불변식 오라클, 바이트 패턴 검색, x86-16 정적 역어셈블, 로더 코드 추적, 컨택트 시트, 채널 순서 실험, 공개 공략 대조), 함정과 노트가 틀린 곳, 합성 픽스처 + 게이트 골든 테스트, 다른 KOEI 게임 체크리스트 | 새 형식을 분석하거나 다른 게임에 적용할 때 |
| [SCENARIO.md](SCENARIO.md) | 시나리오의 흐름: 블록이 하는 일, 선택지·질문·블록 이동, 합류·이탈, 전투 블록의 짜임, 장별 구조와 원작 모드 변환과의 대응. 블록 단위 개요는 [SCENARIO_FLOW.md](SCENARIO_FLOW.md)(원문 없음) | 원작의 장을 변환하거나 이야기 흐름을 확인할 때 |
| [STATUS.md](STATUS.md) | 해독 완료·부분·미해독 목록과 원작 모드까지 남은 단계 | 다음에 무엇을 할지 정할 때 |

## 범위

* **대상**: 한국어 DOS/V판(비스코 유통) `GAME` 폴더. 번체 중문 DOS판은 같은 파일군이라 같은 코드로 읽히도록 만들었지만
  실물로는 확인하지 않았습니다. Steam 2017판·PC-98 디스크 이미지는 판본 식별까지만 합니다.
* **검증 기준**: 사용자가 보유한 한국어 DOS/V 사본 **하나**(2026-09). 다른 사본과 교차 확인하지 못한 사실은 그렇게 적었습니다.
* **다루지 않는 것**: 게임 규칙 수치의 재현(그것은 [../RULES.md](../RULES.md)), 기본 팩 형식(그것은 [../MODDING.md](../MODDING.md)).

## 지킨 원칙 (법적·윤리적)

1. **자기 정품만**: 분석 대상은 저장소 소유자가 직접 보유한 사본이며, 저장소의 `.gitignore`에 등록된 로컬 폴더에만 둡니다.
   원작 파일·디스크 이미지를 배포하거나 구하는 방법을 안내하지 않습니다.
2. **읽기 전용**: 설치 폴더에 아무것도 쓰지 않습니다. 임포터도 출력 폴더가 설치 폴더 안이면 거부합니다.
3. **관찰용 실행만**: v0.2.1(2026-10-01)까지 원작의 실행 파일(`MAIN.EXE`, `TFDED.COM`, `OPEN.EXE` 등)은 **한 번도
   실행하지 않았고**, 그때까지의 결론은 모두 파일을 읽고 정적으로 분석해서 얻었습니다. 그 뒤로는 소유자의 사본을 공식
   DOSBox-X에서 **관찰용으로만** 실행합니다([DECISIONS D22](../DECISIONS.md)). 설치 폴더의 복사본을 쓰고, 복제 방지는 소유자가
   설명서로 답하며 우회하지 않습니다. 패키지에 딸린 바이너리(`DOSBox.exe`, `CRACK.COM`)는 실행하지 않습니다. 관찰로 얻은
   사실은 **[관찰]** 로 표시합니다.
4. **상호운용 목적의 정적 분석만**: 역어셈블은 사용자가 가진 파일을 읽는 호환 구현을 만들기 위한 것이며, 복제 방지를
   우회하지 않습니다. 컨테이너가 암호화되어 있으면 감지·보고에서 멈추고 법률 검토를 먼저 합니다.
5. **클린룸**: 형식 **사실**(구조·크기·수치)만 직접 확인해 구현했습니다. 다른 프로젝트의 코드(라이선스가 없거나
   GPL인 것 포함)는 복사하지 않았고, 공개 조사 노트는 검증할 가설로만 썼습니다.
6. **저장소와 CI에 KOEI 바이트 없음**: 원작 파일의 바이트, 그림·텍스트, 팔레트 색·능력치·이름 목록 같은 데이터 표를
   저장소에 넣지 않습니다. 실행 파일 안의 표는 **읽는 코드의 서명**으로 사용자의 파일에서 찾습니다. 문서에는 구조·오프셋·
   개수·알고리즘과, 코드의 뜻을 알려 주는 짧은 명칭, 공개 공략으로 알려진 확인용 수치만 적습니다.
7. **게이트 골든 테스트**: 모든 단위 테스트는 우리 인코더로 만든 합성 데이터를 씁니다. 실물 검사는 개발자가
   `EIKETSU_ORIGINAL_DIR`로 자기 설치본을 가리킬 때만 돕니다(CI에서는 건너뜀).
8. **추측하지 않음**: 확인하지 못한 형식은 "지원 안 함/미상"으로 보고합니다. 추측으로 그린 결과를 제품 경로에 넣지 않습니다.

## 코드와 형식의 대응

모든 모듈은 `crates/hero-import/src/`에 있습니다.

| 모듈 | 다루는 형식 | 명세 |
|---|---|---|
| `probe.rs` | 공유용 매니페스트(파일 목록·해시·첫 16바이트·컨테이너 요약) | [ORIGINAL_DATA §5](../ORIGINAL_DATA.md#5-매니페스트-공유로-돕기-특히-steam판) |
| `edition.rs`, `diskimage.rs` | `DISK*.R3I` 헤더, 텍스트 통계, Steam 실행기, PC-98 D88/FDI/HDI | [FORMATS §3](FORMATS.md#edition) |
| `install.rs` | 대소문자 무시 파일 찾기, 읽기 전용 접근 | — |
| `ls11.rs` | `LS11`/`Ls11` 아카이브(디렉터리, 코덱, 인코더) | [FORMATS §4](FORMATS.md#ls11) |
| `table6.rs` | 6바이트 테이블(`FACEDAT`, `PACKGRP`) | [FORMATS §5](FORMATS.md#table6) |
| `tfdce.rs` | TF-DCE 이미지(얼굴, `PACKGRP` 화면) | [FORMATS §6](FORMATS.md#tfdce) |
| `palette.rs` | `MAIN.EXE` 팔레트 뱅크 | [FORMATS §7](FORMATS.md#palette) |
| `planar.rs`, `image.rs` | 16×16 셀, 플레인·packed 이미지, 인덱스 PNG | [FORMATS §8](FORMATS.md#planar) |
| `sprites.rs` | 스프라이트·칩 아카이브별 배치, 팔레트 슬롯, 항목 묶음 | [FORMATS §8.2](FORMATS.md#planar) |
| `maps.rs` | `HEXZMAP`, `HEXBMAP`, `MMAP`, `SMAP`, `PMAP`, `MAIN.EXE` 맵 표 서명 | [FORMATS §10](FORMATS.md#maps), [§15](FORMATS.md#main-exe) |
| `text.rs` | EUC-KR/Big5, `SNRnM`, `IPPAN0M` | [FORMATS §11](FORMATS.md#text) |
| `ippan.rs` | `IPPAN0` 색인 | [FORMATS §12](FORMATS.md#ippan) |
| `scenario.rs` | `SNRnD` 바이트코드와 명령 집합 | [FORMATS §13](FORMATS.md#scenario) |
| `bakdata.rs` | `BAKDATA` 마스터 표 | [FORMATS §14](FORMATS.md#bakdata) |
| `extract.rs` | 미디어 오버레이 폴더와 `index.json`, 종류별 상태 | [ORIGINAL_DATA §4.3](../ORIGINAL_DATA.md#43-추출) |
| `testutil.rs` | 합성 설치본(우리 인코더로 만듦) | [METHOD §4](METHOD.md#fixtures) |
| `tests/golden.rs`, `tests/tfdce_golden.rs` | 실물 골든 테스트(`EIKETSU_ORIGINAL_DIR`) | [METHOD §4](METHOD.md#fixtures) |

명령줄은 `crates/hero-tools`의 `hero-tools original probe|extract`입니다.

## 신뢰도 표기

[FORMATS.md](FORMATS.md)의 항목에는 **[검증]**(실물 전체로 확인), **[코드]**(실행 파일의 읽는 코드로 확인),
**[추론]**(모순은 없으나 미확정), **[미상]**, **[외부]**(공개 조사의 주장, 미확인) 중 하나가 붙습니다.
