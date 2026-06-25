# Changelog

## [0.11.3](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.11.2...rusty-jack-v0.11.3) (2026-06-25)


### Bug Fixes

* detect silent event tap and fall back to idle monitor ([#161](https://github.com/the-hcma/rusty-jack/issues/161)) ([641737c](https://github.com/the-hcma/rusty-jack/commit/641737c312377e48ebb9f5292a9b6a1ed2f84aab))

## [0.11.2](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.11.1...rusty-jack-v0.11.2) (2026-06-24)


### Bug Fixes

* address event tap review feedback on PR [#158](https://github.com/the-hcma/rusty-jack/issues/158) ([#159](https://github.com/the-hcma/rusty-jack/issues/159)) ([46868af](https://github.com/the-hcma/rusty-jack/commit/46868afcecb5031839778f19b03f93d2d1f607eb))
* narrow event tap activity monitor and debounce idle wakes ([#158](https://github.com/the-hcma/rusty-jack/issues/158)) ([5ee7fc9](https://github.com/the-hcma/rusty-jack/commit/5ee7fc91e7760eb2cb95cedeedebf03077741ac6))

## [0.11.1](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.11.0...rusty-jack-v0.11.1) (2026-06-22)


### Bug Fixes

* harden ScalarWebAPI SSDP discovery with progress feedback ([#153](https://github.com/the-hcma/rusty-jack/issues/153)) ([a0ba210](https://github.com/the-hcma/rusty-jack/commit/a0ba210b7c3afd2d40837d23527cff44611e0d9d))
* show spinner during all user-visible ScalarWebAPI probing ([#156](https://github.com/the-hcma/rusty-jack/issues/156)) ([3fa7245](https://github.com/the-hcma/rusty-jack/commit/3fa72450152e54e6f9af1bbedfc5750b5a1a06ec))
* skip duplicate release CI wait and fix tap formula verify ([#152](https://github.com/the-hcma/rusty-jack/issues/152)) ([5eb69d5](https://github.com/the-hcma/rusty-jack/commit/5eb69d55bff10702cafe89c4bb191e67a822acf0))

## [0.11.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.10.1...rusty-jack-v0.11.0) (2026-06-22)


### Features

* configure ScalarWebAPI speaker input by human-readable name ([#150](https://github.com/the-hcma/rusty-jack/issues/150)) ([bb94bc1](https://github.com/the-hcma/rusty-jack/commit/bb94bc18c5497994a7646d232462350b23fff85b))


### Bug Fixes

* wait for release PR CI before merge prompt ([#149](https://github.com/the-hcma/rusty-jack/issues/149)) ([c28ad83](https://github.com/the-hcma/rusty-jack/commit/c28ad83c083e030bf937ba06d9c7a5cbe65ca80e))

## [0.10.1](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.10.0...rusty-jack-v0.10.1) (2026-06-22)


### Bug Fixes

* **ci:** delete Graphite MQ staging branches when PR closes ([#132](https://github.com/the-hcma/rusty-jack/issues/132)) ([7b54b34](https://github.com/the-hcma/rusty-jack/commit/7b54b342b1b8e40ff62d244100082c4ebd08d06e))
* **ci:** harden Graphite restack guard push dedup ([#145](https://github.com/the-hcma/rusty-jack/issues/145)) ([4bed8cf](https://github.com/the-hcma/rusty-jack/commit/4bed8cf20cd83ee147f1ac62f8775796c1b4ab0e))
* **ci:** key CI concurrency on head SHA ([#135](https://github.com/the-hcma/rusty-jack/issues/135)) ([5798a8b](https://github.com/the-hcma/rusty-jack/commit/5798a8bf5364c0e27e0547b2375e7eb8c44221aa))
* **ci:** run CI on Graphite restack (push fallback + guard dedup) ([#139](https://github.com/the-hcma/rusty-jack/issues/139)) ([ee1e26f](https://github.com/the-hcma/rusty-jack/commit/ee1e26f6d2ed8c8982ef076cfe19423ea432e903))
* **ci:** skip duplicate push CI when open PR exists ([#144](https://github.com/the-hcma/rusty-jack/issues/144)) ([97a3819](https://github.com/the-hcma/rusty-jack/commit/97a3819c6a44463ef4d6589c3ee3226cb8838ced))
* improve do-release tap publish and release PR merge ([#134](https://github.com/the-hcma/rusty-jack/issues/134)) ([2799c63](https://github.com/the-hcma/rusty-jack/commit/2799c638c2fe3486c1d03005554825903070bd83))

## [0.10.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.9.2...rusty-jack-v0.10.0) (2026-06-11)


### Features

* add do-release script for interactive full release flow ([#131](https://github.com/the-hcma/rusty-jack/issues/131)) ([77c2222](https://github.com/the-hcma/rusty-jack/commit/77c22226f00c25c163b9f250d47ac4edabcb85f4))
* **ci:** add secret-scan job ([#129](https://github.com/the-hcma/rusty-jack/issues/129)) ([8b7cf9f](https://github.com/the-hcma/rusty-jack/commit/8b7cf9f1d626369b75a798ff7d4fd20fad387813))
* clarify idle-to-active activity log lines ([#130](https://github.com/the-hcma/rusty-jack/issues/130)) ([24c9fee](https://github.com/the-hcma/rusty-jack/commit/24c9fee4589543a3fbcf9c0cf516ea459767c0cf))


### Bug Fixes

* **ci:** sync canonical secret-scan script ([#127](https://github.com/the-hcma/rusty-jack/issues/127)) ([911d72b](https://github.com/the-hcma/rusty-jack/commit/911d72b31cd6896a61dd77be9377dc4f801fe4c2))

## [0.9.2](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.9.1...rusty-jack-v0.9.2) (2026-06-10)


### Bug Fixes

* stop restarting eqMac on idle-to-active daemon ticks ([#124](https://github.com/the-hcma/rusty-jack/issues/124)) ([6b7d7e8](https://github.com/the-hcma/rusty-jack/commit/6b7d7e8786d71e9c185835a32f8935491f946297))

## [0.9.1](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.9.0...rusty-jack-v0.9.1) (2026-06-10)


### Bug Fixes

* remove WoL and debounce successful Scalar wake sends ([#122](https://github.com/the-hcma/rusty-jack/issues/122)) ([7f4923c](https://github.com/the-hcma/rusty-jack/commit/7f4923cb518dc2cbc3757e310a0140f02a2bd882))

## [0.9.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.8.0...rusty-jack-v0.9.0) (2026-06-09)


### Features

* keep ScalarWebAPI speakers and eqMac alive while Mac is active ([#120](https://github.com/the-hcma/rusty-jack/issues/120)) ([9535153](https://github.com/the-hcma/rusty-jack/commit/9535153220f73a1719a008c6a5d7da95c0486fcf))

## [0.8.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.7.0...rusty-jack-v0.8.0) (2026-06-09)


### Features

* log purge, signed driver prompts, and ScalarWebAPI refinements ([#117](https://github.com/the-hcma/rusty-jack/issues/117)) ([303f2a2](https://github.com/the-hcma/rusty-jack/commit/303f2a275a9800ae80010621d25d0a92819bc506))


### Bug Fixes

* include tap PR URL in publish-release wait message ([#113](https://github.com/the-hcma/rusty-jack/issues/113)) ([180c949](https://github.com/the-hcma/rusty-jack/commit/180c949a3cb7a35863339a8ad34d256b2fa884c7))

## [0.7.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.6.3...rusty-jack-v0.7.0) (2026-06-08)


### Features

* log daemon version on startup ([#111](https://github.com/the-hcma/rusty-jack/issues/111)) ([8a55726](https://github.com/the-hcma/rusty-jack/commit/8a55726fa549cf4699fd8751b1eccd38407add95))


### Bug Fixes

* print PR merged line once at end of publish-release ([#110](https://github.com/the-hcma/rusty-jack/issues/110)) ([275715d](https://github.com/the-hcma/rusty-jack/commit/275715deadc36ab544eff09c3994485dbf01daef))

## [0.6.3](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.6.2...rusty-jack-v0.6.3) (2026-06-08)


### Bug Fixes

* gate native driver install until signed release ships ([#105](https://github.com/the-hcma/rusty-jack/issues/105)) ([60349ed](https://github.com/the-hcma/rusty-jack/commit/60349edaec22eba6bbe725d18c5629304fe05b32))
* harden CI checkout against ephemeral merge refs ([#107](https://github.com/the-hcma/rusty-jack/issues/107)) ([a5784fc](https://github.com/the-hcma/rusty-jack/commit/a5784fc61a3f675eb3b54e5d2e02109f4ce6b935))
* print tap PR URL before waiting for merge in publish-release ([#108](https://github.com/the-hcma/rusty-jack/issues/108)) ([57454cc](https://github.com/the-hcma/rusty-jack/commit/57454cc22c4926dd66dbf25fc906d7354cd77f9f))

## [0.6.2](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.6.1...rusty-jack-v0.6.2) (2026-06-08)


### Bug Fixes

* add retries to release PR URL lookup and tap publish wait ([#103](https://github.com/the-hcma/rusty-jack/issues/103)) ([74f8e86](https://github.com/the-hcma/rusty-jack/commit/74f8e86074aeb076fb8c3a698bcf82f93daf2488))
* show running daemon version in upgrade output ([#104](https://github.com/the-hcma/rusty-jack/issues/104)) ([a1e945e](https://github.com/the-hcma/rusty-jack/commit/a1e945ea3cd42e7820d0b61608351bec21f30dfa))

## [0.6.1](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.6.0...rusty-jack-v0.6.1) (2026-06-08)


### Bug Fixes

* detect stale daemon via LaunchAgent version env ([#101](https://github.com/the-hcma/rusty-jack/issues/101)) ([7cad47b](https://github.com/the-hcma/rusty-jack/commit/7cad47be2939f0196e7b28a0086600ba47052ec4))
* print release PR URL last in update-release-pr ([#100](https://github.com/the-hcma/rusty-jack/issues/100)) ([5f0a417](https://github.com/the-hcma/rusty-jack/commit/5f0a4173817f32aac604e785fd0c711bd12f2e4c))

## [0.6.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.5.0...rusty-jack-v0.6.0) (2026-06-08)


### Features

* flag stale daemon in status and brew post_install guidance ([#98](https://github.com/the-hcma/rusty-jack/issues/98)) ([bd18b89](https://github.com/the-hcma/rusty-jack/commit/bd18b89e93890957cdb141f63c54f536b6ef767d))

## [0.5.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.4.1...rusty-jack-v0.5.0) (2026-06-08)


### Features

* show ScalarWebAPI model in list and status ([#96](https://github.com/the-hcma/rusty-jack/issues/96)) ([69208df](https://github.com/the-hcma/rusty-jack/commit/69208df043cc364aba5804586825370157728f00))

## [0.4.1](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.4.0...rusty-jack-v0.4.1) (2026-06-08)


### Bug Fixes

* auto fast-forward main in release scripts ([#94](https://github.com/the-hcma/rusty-jack/issues/94)) ([8bcfeb8](https://github.com/the-hcma/rusty-jack/commit/8bcfeb805776619687c865b5890e803e163d49d4))

## [0.4.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.3.0...rusty-jack-v0.4.0) (2026-06-08)


### Features

* auto-discover ScalarWebAPI devices during install and reconfigure ([#92](https://github.com/the-hcma/rusty-jack/issues/92)) ([7594371](https://github.com/the-hcma/rusty-jack/commit/7594371c266ea779688948bc275c60f1c69fe622))


### Bug Fixes

* fix make uninstall and embed git commit in version ([#91](https://github.com/the-hcma/rusty-jack/issues/91)) ([a6a0459](https://github.com/the-hcma/rusty-jack/commit/a6a04591206b3a504f2d7008b955bf463226392c))

## [0.3.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.2.0...rusty-jack-v0.3.0) (2026-06-08)


### Features

* add activity poll logging and status ([#87](https://github.com/the-hcma/rusty-jack/issues/87)) ([b928f86](https://github.com/the-hcma/rusty-jack/commit/b928f86924d1a9188775ef001aad02902e4518fa))
* add daemon logging ([#86](https://github.com/the-hcma/rusty-jack/issues/86)) ([e0a2b6b](https://github.com/the-hcma/rusty-jack/commit/e0a2b6ba43931c16dcd779401312a29c5864f0fc))
* show daemon log paths in status and improve brew caveats ([#85](https://github.com/the-hcma/rusty-jack/issues/85)) ([16e5b8d](https://github.com/the-hcma/rusty-jack/commit/16e5b8d7556f4d88621db91cdf0001c06dbb0030))


### Bug Fixes

* harden publish-release repair and tap PR handling ([#83](https://github.com/the-hcma/rusty-jack/issues/83)) ([f8f5c3d](https://github.com/the-hcma/rusty-jack/commit/f8f5c3df8ebcfb663f8b3e0d70a9589f6516d89f))
* resolve release-please tag names in publish-release ([#81](https://github.com/the-hcma/rusty-jack/issues/81)) ([e39da48](https://github.com/the-hcma/rusty-jack/commit/e39da48e3d93c0d8eec05924d6f8142099b80ccc))
* use SSDP endpoint only and reduce redundant ScalarWebAPI wakes ([#89](https://github.com/the-hcma/rusty-jack/issues/89)) ([f9c9d56](https://github.com/the-hcma/rusty-jack/commit/f9c9d5653c3a93a8b45ad2868ffff42e9f550ebd))

## [0.2.0](https://github.com/the-hcma/rusty-jack/compare/rusty-jack-v0.1.1...rusty-jack-v0.2.0) (2026-06-08)


### Features

* add config init/validate commands ([#45](https://github.com/the-hcma/rusty-jack/issues/45)) ([e5e107b](https://github.com/the-hcma/rusty-jack/commit/e5e107b68dd50f35afb6eacd3995b3a4e537974a))
* add driver swap workflow ([#24](https://github.com/the-hcma/rusty-jack/issues/24)) ([f62428c](https://github.com/the-hcma/rusty-jack/commit/f62428c80be8f51cf26176c7ee8926900671ca30))
* add HDMI DisplayPort driver lifecycle ([#17](https://github.com/the-hcma/rusty-jack/issues/17)) ([370b66c](https://github.com/the-hcma/rusty-jack/commit/370b66c3e39c45db78602bde41f602fd462653f3))
* add passthrough planning skeleton for Phase 7 ([#26](https://github.com/the-hcma/rusty-jack/issues/26)) ([14c51a8](https://github.com/the-hcma/rusty-jack/commit/14c51a8847e360853536fb01c4b663837d18448e))
* confirm install reconfigure diff and prompt ScalarWebAPI triggers ([#36](https://github.com/the-hcma/rusty-jack/issues/36)) ([d17a6c2](https://github.com/the-hcma/rusty-jack/commit/d17a6c244305f92486a9f639b20aafe917f69af7))
* expose native virtual output device ([#21](https://github.com/the-hcma/rusty-jack/issues/21)) ([6078f19](https://github.com/the-hcma/rusty-jack/commit/6078f19226cee25f13035efca1f77570dfe2e21b))
* HAL driver smoke, eqMac restore, and signing helpers ([#44](https://github.com/the-hcma/rusty-jack/issues/44)) ([18dafed](https://github.com/the-hcma/rusty-jack/commit/18dafedb8280a9f2ee893509f97317a25e6728ba))
* native HDMI/DP software volume passthrough ([#42](https://github.com/the-hcma/rusty-jack/issues/42)) ([75dc556](https://github.com/the-hcma/rusty-jack/commit/75dc556ad10c87d710a6544a5bb154edde6e437c))
* offer reconfigure when install finds existing config ([#35](https://github.com/the-hcma/rusty-jack/issues/35)) ([c6a0e4e](https://github.com/the-hcma/rusty-jack/commit/c6a0e4e66885aedaea7d1fedf97eb64a822b34a0))
* package native HAL driver bundle ([#18](https://github.com/the-hcma/rusty-jack/issues/18)) ([7fe259e](https://github.com/the-hcma/rusty-jack/commit/7fe259e88e180f5f0c0846a94e7e747c691a6767))
* prompt ScalarWebAPI wake triggers during install ([#29](https://github.com/the-hcma/rusty-jack/issues/29)) ([5f70de9](https://github.com/the-hcma/rusty-jack/commit/5f70de9eb4267e282574a95d58cdafa8e0016c8d))
* restore pre-install output on uninstall ([#46](https://github.com/the-hcma/rusty-jack/issues/46)) ([152335b](https://github.com/the-hcma/rusty-jack/commit/152335b36388c6fa3444659ac1665ed5ac35656c))
* show ScalarWebAPI power state in status ([#48](https://github.com/the-hcma/rusty-jack/issues/48)) ([3ccbc20](https://github.com/the-hcma/rusty-jack/commit/3ccbc2030fa94c648aab1f8744b91f984e0244b4))
* validate preferred monitor identity ([#14](https://github.com/the-hcma/rusty-jack/issues/14)) ([1d292c3](https://github.com/the-hcma/rusty-jack/commit/1d292c3cfb65bba1cdb71142700a51f5cc9697aa))
* wake daemon on CoreAudio property changes ([#47](https://github.com/the-hcma/rusty-jack/issues/47)) ([91f9340](https://github.com/the-hcma/rusty-jack/commit/91f934052612d8ead81bfb480a52c5799e0bb14a))


### Bug Fixes

* add AppLaunch error variant for non-launchd failures ([#52](https://github.com/the-hcma/rusty-jack/issues/52)) ([69949e2](https://github.com/the-hcma/rusty-jack/commit/69949e258056bb940425738514211abc22a78239))
* atomically rewrite config on canonicalization ([#53](https://github.com/the-hcma/rusty-jack/issues/53)) ([398ae30](https://github.com/the-hcma/rusty-jack/commit/398ae3077bf95f82f8f7a2eeffd832a24fd62e58))
* avoid exists() pre-checks in state persistence ([#55](https://github.com/the-hcma/rusty-jack/issues/55)) ([286e6b3](https://github.com/the-hcma/rusty-jack/commit/286e6b3499849ade0e692f1532fedb9623179395))
* check eqMac health during daemon no-op ticks ([#6](https://github.com/the-hcma/rusty-jack/issues/6)) ([d238133](https://github.com/the-hcma/rusty-jack/commit/d23813327f48e445d15aa1e78410575ae78a2a90))
* **ci:** run CI for stacked PR base branches ([#31](https://github.com/the-hcma/rusty-jack/issues/31)) ([553776e](https://github.com/the-hcma/rusty-jack/commit/553776ef9923f02899b6f977a82f8aed67526e05))
* clarify upgrade and driver recommendations ([#22](https://github.com/the-hcma/rusty-jack/issues/22)) ([3cc279b](https://github.com/the-hcma/rusty-jack/commit/3cc279bfde3dd1d513be9eb62703d88e4cb271fa))
* confirm resume after picker override ([#25](https://github.com/the-hcma/rusty-jack/issues/25)) ([9c9115c](https://github.com/the-hcma/rusty-jack/commit/9c9115c10ac5f3eece0ef2ae06447cca3f1a4d89))
* defer ScalarWebAPI wake until network is reachable ([#30](https://github.com/the-hcma/rusty-jack/issues/30)) ([fee3058](https://github.com/the-hcma/rusty-jack/commit/fee3058ebaa5241d99eee9798fc26c4b95e47d31))
* gate Sony fallback on network changes ([#12](https://github.com/the-hcma/rusty-jack/issues/12)) ([af59b54](https://github.com/the-hcma/rusty-jack/commit/af59b54e49e2a85e32a519f631d5b8e19635c791))
* improve install reconfigure UX and ScalarWebAPI docs ([#38](https://github.com/the-hcma/rusty-jack/issues/38)) ([cf1bd71](https://github.com/the-hcma/rusty-jack/commit/cf1bd71f87d5e7b322c39715a14db487a7d36645))
* inject persistence paths via RUSTY_JACK_STATE_DIR ([#60](https://github.com/the-hcma/rusty-jack/issues/60)) ([bde2f9f](https://github.com/the-hcma/rusty-jack/commit/bde2f9f3989b19e2a3824b06fa9f22440c231756))
* keep config keys sorted ([#10](https://github.com/the-hcma/rusty-jack/issues/10)) ([4cd23fb](https://github.com/the-hcma/rusty-jack/commit/4cd23fb4c53e2a92a34fc03bb4080912835a59e2))
* log unverified startup volume ensure results ([#58](https://github.com/the-hcma/rusty-jack/issues/58)) ([1a39638](https://github.com/the-hcma/rusty-jack/commit/1a3963890ffbf1649c5e03a5cdaff8043df151c7))
* parse config JSON once per load ([#54](https://github.com/the-hcma/rusty-jack/issues/54)) ([f6ecf7b](https://github.com/the-hcma/rusty-jack/commit/f6ecf7b08e8e5f3769cdd4731a6164dc106d19fe))
* poll for eqMac readiness instead of fixed sleeps ([#62](https://github.com/the-hcma/rusty-jack/issues/62)) ([afc3ee5](https://github.com/the-hcma/rusty-jack/commit/afc3ee5523c018a87fe87e6eaf364cae3d35eec1))
* prefer friendly device labels in policy status output ([#39](https://github.com/the-hcma/rusty-jack/issues/39)) ([16f2d1e](https://github.com/the-hcma/rusty-jack/commit/16f2d1e0177984b5c759da0462c0c9438377c368))
* prevent daemon audio route flicker ([#19](https://github.com/the-hcma/rusty-jack/issues/19)) ([f3dd04b](https://github.com/the-hcma/rusty-jack/commit/f3dd04b1f7720e04593473e90ff1ef10436b68bd))
* rebuild release binary when HEAD changes ([#74](https://github.com/the-hcma/rusty-jack/issues/74)) ([d94317b](https://github.com/the-hcma/rusty-jack/commit/d94317bc09efe891c58381c62753c5d62f009b7f))
* recover stale eqMac after wake ([#15](https://github.com/the-hcma/rusty-jack/issues/15)) ([a68b49e](https://github.com/the-hcma/rusty-jack/commit/a68b49e25576cd88272acdf7dbdb46e1b3959f2f))
* simplify alive output device selection ([#59](https://github.com/the-hcma/rusty-jack/issues/59)) ([e4ae828](https://github.com/the-hcma/rusty-jack/commit/e4ae82898970dd8ff1ac6aa66f0964dad57c2310))
* snapshot installed version before make upgrade overwrites binary ([#75](https://github.com/the-hcma/rusty-jack/issues/75)) ([92c4887](https://github.com/the-hcma/rusty-jack/commit/92c488704c6e023b72b1e34f1abd68a9dd29ebd0))
* stabilize daemon routing and volume handling ([#5](https://github.com/the-hcma/rusty-jack/issues/5)) ([0f7297b](https://github.com/the-hcma/rusty-jack/commit/0f7297b071d2c990f8614bd6286a8584e9d39de6))
* treat CONNECTION_REQUIRED as unreachable for ScalarWebAPI wake ([#57](https://github.com/the-hcma/rusty-jack/issues/57)) ([cbf4350](https://github.com/the-hcma/rusty-jack/commit/cbf4350eae1d9e7b157663ff6ced28c860023695))
* use CGEventSource for macOS idle time ([#61](https://github.com/the-hcma/rusty-jack/issues/61)) ([0e02ea5](https://github.com/the-hcma/rusty-jack/commit/0e02ea51756cfb463c0f8865532c78e7c63c1343))
* wake Sony speaker on daemon startup ([#9](https://github.com/the-hcma/rusty-jack/issues/9)) ([2eed149](https://github.com/the-hcma/rusty-jack/commit/2eed149c15723bed27adb37763a14475d7a73421))
* warn and keep last config on scheduled reload failure ([#56](https://github.com/the-hcma/rusty-jack/issues/56)) ([6bfccc5](https://github.com/the-hcma/rusty-jack/commit/6bfccc5e229e7439c708d6363582d2decf7a745f))

## [0.1.1](https://github.com/the-hcma/rusty-jack/releases/tag/v0.1.1) (2026-05-25)

Initial public release published through the `the-hcma/tap` Homebrew tap.
