# 뿌요뿌요!! 20주년 기념판 (NDS) 한글 패처

닌텐도 DS용 《뿌요뿌요!! 20주년 기념판》(ぷよぷよ！！) 일본판에 한글 패치를 적용하는 Rust 코드입니다. NDS ROM과 NitroFS(FNT/FAT), NARC, LZ11·BLZ 압축, MTX·스토리 대사 형식의 읽기·쓰기, 한글 폰트와 라벨·스프라이트·화면 그림 생성, 고정 크기 ARM9 재압축, 배너 제목 교체, 기대 바이트와 보호 영역을 확인하는 제품 빌드를 제공합니다.

배포용 패치와 적용 방법은 [뿌요뿌요 시리즈 한글 패치](https://github.com/mcpads/puyo-puyo-kr-patch/tree/main/nds-puyo20)에서 제공합니다.

## 제공하지 않는 것

이 저장소에는 원본 ROM, 패치를 적용한 ROM, 번역 JSON, 번역 그래픽과 그 명세, 폰트 파일이 없습니다. 따라서 이 저장소만으로는 배포 패치를 다시 만들 수 없습니다. 아래 입력을 직접 갖춘 경우에만 `build-product`가 ROM을 생성합니다.

## 빌드와 테스트

```bash
cargo build --release --locked
cargo test --locked
```

기본 테스트는 합성 입력만 사용합니다. 폰트, 번역 JSON과 그래픽 명세가 필요한 테스트는 `#[ignore = "requires ..."]`로 필요한 입력을 밝혀 두었습니다. 입력을 갖춘 뒤 `cargo test -- --ignored`로 실행하며, 입력이 없으면 성공으로 넘어가지 않고 실패합니다.

## 지원 원본

| 원본 | 게임 코드 | 크기 | SHA-256 |
| --- | --- | --- | --- |
| 일본판 | TP4J (리비전 0) | 67,108,864바이트 | `6b8227780eea751aa22b4201aea63c36d8844a994791109708d9fe2df399944d` |

`build-product`는 원본의 크기와 SHA-256을 [config/source.json](config/source.json)과 대조하고, 다르면 진행하지 않습니다. 원본 확인만 하려면 `cargo run --release -- verify-source path/to/japanese.nds`를 실행합니다.

## 빌드 입력

[config/build.json](config/build.json)이 제품 빌드의 유일한 명세입니다. 214개 구성요소마다 준비 명령, 원천 파일, 폰트 역할을 적고, 여러 구성요소가 같은 NARC 멤버를 쓰면 `owners`가 소유자를 정합니다. 경로는 `config/` 기준입니다.

| 입력 | 경로 | 비고 |
| --- | --- | --- |
| 번역 JSON | `assets/translations/*.json` | 구성요소별 `translation` |
| 그래픽 명세와 원화 | `assets/art/` | 구성요소별 `spec`, `translation`, `artwork` |
| 폰트 | `assets/fonts/<이름>/` | 아래 표 |

빌드는 폰트의 SHA-256이 명세와 다르면 진행하지 않습니다. 배포 패치 v1.0.0은 다음 파일로 만들었습니다.

| 경로 | 배포처 | SHA-256 |
| --- | --- | --- |
| `assets/fonts/galmuri11/Galmuri11.ttf` | [Galmuri](https://github.com/quiple/galmuri) v2.40.3 릴리스 zip | `2c709890595668f7bdb6df408420fda957dde0288e95b31a1cc17a2ab98b4b4f` |
| `assets/fonts/galmuri11-bold/Galmuri11-Bold.ttf` | Galmuri v2.40.3 | `f899e7d8d646a1990a4b6260caa6631b3a50b567ea605ab0047d2d3a955deedb` |
| `assets/fonts/galmuri11-condensed/Galmuri11-Condensed.ttf` | Galmuri v2.40.3 릴리스 zip | `7b433b4a007c36dfb535fdea11de3e4f4c8b641ab591ed05d3bc0a4bbd75eb5f` |
| `assets/fonts/galmuri14/Galmuri14.ttf` | Galmuri | `d3818c0f2898a3b2d79ccd04ec1e4de5e8940aa26abee261f73e315a44ce8df9` |
| `assets/fonts/galmuri9/Galmuri9.ttf` | Galmuri v2.40.3 | `48eaa0efdff031598ce1d0162e08cb8f53e8b666064bd5cdc2b902d508f231ee` |
| `assets/fonts/galmuri7/Galmuri7.ttf` | Galmuri v2.40.3 | `1be9a0e60647fe919ed57eb1aaf4da2e188c654033bea51ad6010d1507880396` |
| `assets/fonts/galmurimono11/GalmuriMono11.ttf` | Galmuri | `2380e9cc83e03a71f6abecbdbcad226061e3483ae19d4265f28797d4be73c2e5` |
| `assets/fonts/denkichip/x10y12pxDenkiChipHangul.ttf` | [x10y12pxDenkiChipHangul](https://github.com/quiple/x10y12pxDenkiChipHangul) v1.213 | `4589cb1a59bcbd669ad7ac0669827e5a4d411832048e9bdd9618c907c1a8d272` |
| `assets/fonts/bmjua/BMJUA.ttf` | 배달의민족 주아체 | `e8e6aa8b1b662c7bf0d7f136f29e822e0985176458a6e5d0ba08afc4a5c901a9` |

## ROM 생성

```bash
cargo build --release --locked
target/release/nds-puyo20 build-product path/to/japanese.nds \
  --spec config/build.json --out out/product
```

출력 디렉터리에 `development.nds`와 빌드 기록 `build.json`(명세·원천 해시, 멤버별 작성 구성요소)이 생깁니다. 출력 디렉터리가 이미 있으면 실패합니다. 같은 입력이면 같은 ROM이 나옵니다.

배포용 xdelta는 다음처럼 만듭니다.

```bash
xdelta3 -e -9 -S none -A -f -s path/to/japanese.nds out/product/development.nds out/product.xdelta
```

위 입력으로 만든 ROM은 배포 패치 v1.0.0을 적용한 ROM과 같습니다(SHA-256 `045342d6229cf81bb4904bc23a00179cce84d907ad70b80471118ac905bf410a`).

## 그 밖의 명령

`identify`, `build`, `merge-archives`와 `prepare-*`·`inspect-*` 명령의 사용법은 `cargo run -- help <명령>`으로 확인할 수 있습니다. `inspect-*`·`check-*` 등 일부 명령은 에뮬레이터 메모리 덤프나 영어판 ROM 같은 별도 입력을 받습니다.

## 라이선스

이 저장소의 소스 코드는 [MIT License](LICENSE)로 제공합니다.
