# M2 candidate dependency closure

Status: **candidate; G-01 pending owner review**. On 2026-11-13 the owner authorized compatibility testing with this closure, without approving it as the formal dependency baseline. Exact direct versions remain those in the decision register.

Generated from `cargo metadata --all-features --locked --format-version 1` and `Cargo.lock` on Windows x86_64 MSVC. This is the all-target resolver graph (including platform-specific packages), not evidence that every listed package was built on this host. The configured Cargo registry replacement downloaded archives from the USTC mirror; canonical lockfile sources remain crates.io.

## Root features

| Feature | Members |
| --- | --- |
| `config-toml` | `dep:toml` |
| `default` | `render-cpu, render-gpu` |
| `read-geojson` | `dep:geojson` |
| `read-geotiff` | `dep:tiff` |
| `read-shapefile` | `dep:shapefile` |
| `render-cpu` | `dep:vello_cpu, dep:vello_common, dep:rayon` |
| `render-gpu` | `dep:vello_gpu, dep:vello_common, dep:wgpu` |
| `rust-proj` | `dep:proj4rs, dep:proj4wkt, dep:crs-definitions` |

## Package versions, sources, checksums, licenses and unified features

License expressions are package declarations; bundled notices and source contents require review. Checksums are the Cargo archive checksums from the lockfile, not guessed hashes or repository commits. Unified features include platform edges from the all-target graph.

| Package | Version | Canonical source | Archive SHA-256 | Declared license | Repository | Unified features |
| --- | --- | --- | --- | --- | --- | --- |
| `addr2line` | `0.24.2` | `registry+https://github.com/rust-lang/crates.io-index` | `dfbe277e56a376000877090da837660b4427aad530e3028d44e0bffe4f89a1c1` | Apache-2.0 OR MIT | https://github.com/gimli-rs/addr2line | `-` |
| `adler2` | `2.0.1` | `registry+https://github.com/rust-lang/crates.io-index` | `320119579fcad9c21884f5c4861d16174d0e06250625266f50fe6898340abefa` | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 | `-` |
| `aho-corasick` | `1.1.3` | `registry+https://github.com/rust-lang/crates.io-index` | `8e60d3430d3a69478ad0993f19238d2df97c507009a52b3c10addcd7f6bcb916` | Unlicense OR MIT | https://github.com/BurntSushi/aho-corasick | `perf-literal, std` |
| `allocator-api2` | `0.2.21` | `registry+https://github.com/rust-lang/crates.io-index` | `683d7910e743518b0e34f1186f92494becacb047c7b6bf616c96772180fef923` | MIT OR Apache-2.0 | https://github.com/zakarumych/allocator-api2 | `alloc` |
| `android_system_properties` | `0.1.5` | `registry+https://github.com/rust-lang/crates.io-index` | `819e7219dbd41043ac279b19830f2efc897156490d7fd6ea916720117ee66311` | MIT/Apache-2.0 | https://github.com/nical/android_system_properties | `-` |
| `annotate-snippets` | `0.12.18` | `registry+https://github.com/rust-lang/crates.io-index` | `0258346f92c78dbc5b336b9a5b27c64447aa10643bc64e651db2edb145361564` | MIT OR Apache-2.0 | https://github.com/rust-lang/annotate-snippets-rs | `default, std` |
| `anstyle` | `1.0.14` | `registry+https://github.com/rust-lang/crates.io-index` | `940b3a0ca603d1eade50a4846a2afffd5ef57a9feac2c0e2ec2e14f9ead76000` | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle.git | `std` |
| `approx` | `0.5.1` | `registry+https://github.com/rust-lang/crates.io-index` | `cab112f0a86d568ea0e627cc1d6be74a1e9cd55214684db5561995f6dad897c6` | Apache-2.0 | https://github.com/brendanzab/approx | `std` |
| `arrayvec` | `0.7.6` | `registry+https://github.com/rust-lang/crates.io-index` | `7c02d123df017efcdfbd739ef81735b36c5ba83ec3c59c80a9d7ecc718f92e50` | MIT OR Apache-2.0 | https://github.com/bluss/arrayvec | `-` |
| `as-slice` | `0.1.5` | `registry+https://github.com/rust-lang/crates.io-index` | `45403b49e3954a4b8428a0ac21a4b7afadccf92bfd96273f1a58cd4812496ae0` | MIT OR Apache-2.0 | https://github.com/japaric/as-slice | `-` |
| `ascii-canvas` | `4.0.0` | `registry+https://github.com/rust-lang/crates.io-index` | `ef1e3e699d84ab1b0911a1010c5c106aa34ae89aeac103be5ce0c3859db1e891` | Apache-2.0/MIT | https://github.com/lalrpop/ascii-canvas | `-` |
| `ash` | `0.38.0+1.3.281` | `registry+https://github.com/rust-lang/crates.io-index` | `0bb44936d800fea8f016d7f2311c6a4f97aebd5dc86f09906139ec848cf3a46f` | MIT OR Apache-2.0 | https://github.com/ash-rs/ash | `debug, default, libloading, loaded, std` |
| `atomic-polyfill` | `1.0.3` | `registry+https://github.com/rust-lang/crates.io-index` | `8cf2bce30dfe09ef0bfaef228b9d414faaf7e563035494d7fe092dba54b300f4` | MIT OR Apache-2.0 | https://github.com/embassy-rs/atomic-polyfill | `-` |
| `autocfg` | `1.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `ace50bade8e6234aa140d9a2f552bbee1db4d353f69b8217bc503490fc1a9f26` | Apache-2.0 OR MIT | https://github.com/cuviper/autocfg | `-` |
| `backtrace` | `0.3.75` | `registry+https://github.com/rust-lang/crates.io-index` | `6806a6321ec58106fea15becdad98371e28d92ccbc7c8f1b3b6dd724fe8f1002` | MIT OR Apache-2.0 | https://github.com/rust-lang/backtrace-rs | `default, std` |
| `bit-set` | `0.10.0` | `registry+https://github.com/rust-lang/crates.io-index` | `09ec2f926cc3060f09db9ebc5b52823d85268d24bb917e472c0c4bea35780a7d` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-set | `-` |
| `bit-set` | `0.8.0` | `registry+https://github.com/rust-lang/crates.io-index` | `08807e080ed7f9d5433fa9b275196cfc35414f66a0c79d864dc51a0d825231a3` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-set | `-` |
| `bit-vec` | `0.8.0` | `registry+https://github.com/rust-lang/crates.io-index` | `5e764a1d40d510daf35e07be9eb06e75770908c27d411ee6c92109c9840eaaf7` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-vec | `-` |
| `bit-vec` | `0.9.1` | `registry+https://github.com/rust-lang/crates.io-index` | `b71798fca2c1fe1086445a7258a4bc81e6e49dcd24c8d0dd9a1e57395b603f51` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-vec | `-` |
| `bitflags` | `2.9.1` | `registry+https://github.com/rust-lang/crates.io-index` | `1b8e56985ec62d17e9c1001dc89c88ecd7dc08e47eba5ec7c29c7b5eeecde967` | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags | `std` |
| `block-buffer` | `0.10.4` | `registry+https://github.com/rust-lang/crates.io-index` | `3078c7629b62d3f0439517fa394996acacc5cbc91c5a20d8c658e77abd503a71` | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils | `-` |
| `bufrw` | `0.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `394c22b416e3981206df472c3bcbbf9ae130883619aefc2199e6087517af1bba` | MIT | https://github.com/tmontaigu/bufrw | `-` |
| `bumpalo` | `3.18.1` | `registry+https://github.com/rust-lang/crates.io-index` | `793db76d6187cd04dff33004d8e6c9cc4e05cd330500379d2394209271b4aeee` | MIT OR Apache-2.0 | https://github.com/fitzgen/bumpalo | `default` |
| `bytemuck` | `1.25.2` | `registry+https://github.com/rust-lang/crates.io-index` | `95832e849adfb21180ccb6826a99da14e5d266ae5c2e668e1602cf234f153797` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck | `bytemuck_derive, derive, extern_crate_alloc, min_const_generics` |
| `bytemuck_derive` | `1.12.1` | `registry+https://github.com/rust-lang/crates.io-index` | `6a1f896587b6f2c069c73d2f0913e2d590c3990285cd2f0b6aa02b786b4c679c` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck | `-` |
| `byteorder` | `1.5.0` | `registry+https://github.com/rust-lang/crates.io-index` | `1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b` | Unlicense OR MIT | https://github.com/BurntSushi/byteorder | `default, std` |
| `cfg-if` | `1.0.1` | `registry+https://github.com/rust-lang/crates.io-index` | `9555578bc9e57714c812a1f84e4fc5b4d21fcb063490c624de019f7464c91268` | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if | `-` |
| `cfg_aliases` | `0.2.2` | `registry+https://github.com/rust-lang/crates.io-index` | `f079e83a288787bcd14a6aea84cee5c87a67c5a3e660c30f557a3d24761b3527` | MIT | https://github.com/katharostech/cfg_aliases | `-` |
| `codepage` | `0.1.2` | `registry+https://github.com/rust-lang/crates.io-index` | `48f68d061bc2828ae826206326e61251aca94c1e4a5305cf52d9138639c918b4` | Apache-2.0 OR MIT | https://github.com/hsivonen/codepage | `-` |
| `codespan-reporting` | `0.13.1` | `registry+https://github.com/rust-lang/crates.io-index` | `af491d569909a7e4dee0ad7db7f5341fef5c614d5b8ec8cf765732aba3cff681` | Apache-2.0 | https://github.com/brendanzab/codespan | `-` |
| `color` | `0.3.3` | `registry+https://github.com/rust-lang/crates.io-index` | `2ec7c5eb7a16992b1904d76c517d170ab353b0e0b3d5a0c81a8a0cd1037893cf` | Apache-2.0 OR MIT | https://github.com/linebender/color | `bytemuck, std` |
| `console_log` | `1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `86919cef3e37b9356ccf54d4421208c17ecfda01beae61393e7ffd72916c0ef1` | MIT/Apache-2.0 | https://github.com/iamcodemaker/console_log | `default` |
| `convert_case` | `0.10.0` | `registry+https://github.com/rust-lang/crates.io-index` | `633458d4ef8c78b72454de2d54fd6ab2e60f9e02be22f3c6104cdc8a4e0fceb9` | MIT | https://github.com/rutrum/convert-case | `-` |
| `cpufeatures` | `0.2.17` | `registry+https://github.com/rust-lang/crates.io-index` | `59ed5838eebb26a2bb2e58f6d5b5316989ae9d08bab10e0e6d103e656d1b0280` | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils | `-` |
| `crc32fast` | `1.5.5` | `registry+https://github.com/rust-lang/crates.io-index` | `2b929dd2a135771f40afabd1073f85eed59c800b7c61901e5c254270497dad64` | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast | `std` |
| `critical-section` | `1.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `790eea4361631c5e7d22598ecd5723ff611904e3344ce8720784c93e3d83d40b` | MIT OR Apache-2.0 | https://github.com/rust-embedded/critical-section | `-` |
| `crossbeam-channel` | `0.5.17` | `registry+https://github.com/rust-lang/crates.io-index` | `98b0cc327b5bc766e7fda9c9260cc0fa81b43a8e240440422dff70788e3f9ef1` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | `default, std` |
| `crossbeam-deque` | `0.8.6` | `registry+https://github.com/rust-lang/crates.io-index` | `9dd111b7b7f7d55b72c0a6ae361660ee5853c9af73f70c3c2ef6858b950e2e51` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | `default, std` |
| `crossbeam-epoch` | `0.9.18` | `registry+https://github.com/rust-lang/crates.io-index` | `5b82ac4a3c2ca9c3460964f020e1402edd5753411d7737aa39c3714ad1b5420e` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | `alloc, std` |
| `crossbeam-utils` | `0.8.21` | `registry+https://github.com/rust-lang/crates.io-index` | `d0a5c400df2834b80a4c3327b3aad3a4c4cd4de0629063962b03235697506a28` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | `default, std` |
| `crs-definitions` | `0.6.0` | `registry+https://github.com/rust-lang/crates.io-index` | `ed764c927c2a19ec1cb85deb88e93560872c04653b0e9e63fbdf6a8edf968bcb` | CC0-1.0 | https://tangled.org/rwell.org/crs-definitions | `proj4, wkt` |
| `crunchy` | `0.2.3` | `registry+https://github.com/rust-lang/crates.io-index` | `43da5946c66ffcc7745f48db692ffbb10a83bfe0afd96235c5c2a4fb23994929` | MIT | https://github.com/eira-fransham/crunchy | `default, limit_128` |
| `crypto-common` | `0.1.6` | `registry+https://github.com/rust-lang/crates.io-index` | `1bfb12502f3fc46cca1bb51ac28df9d618d813cdc3d2f25b9fe775a34af26bb3` | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits | `-` |
| `dbase` | `0.8.0` | `registry+https://github.com/rust-lang/crates.io-index` | `abde818b4b82df61e01348c2b4610c221bcecb97cb7f567da086edf295d8d126` | MIT | https://github.com/tmontaigu/dbase-rs | `encoding_rs` |
| `deranged` | `0.5.9` | `registry+https://github.com/rust-lang/crates.io-index` | `9e9de72ce2ad1f90dc62fa25f0f430ef85eb4b0d8fa0be4f30373bc40a21d28e` | MIT OR Apache-2.0 | https://github.com/jhpratt/deranged | `default` |
| `derive_more` | `2.1.1` | `registry+https://github.com/rust-lang/crates.io-index` | `d751e9e49156b02b44f9c1815bcb94b984cdcc4396ecc32521c739452808b134` | MIT | https://github.com/JelteF/derive_more | `as_ref, constructor, default, deref, deref_mut, display, from, is_variant, std, try_unwrap, unwrap` |
| `derive_more-impl` | `2.1.1` | `registry+https://github.com/rust-lang/crates.io-index` | `799a97264921d8623a957f6c3b9011f3b5492f557bbb7a5a19b7fa6d06ba8dcb` | MIT | https://github.com/JelteF/derive_more | `as_ref, constructor, default, deref, deref_mut, display, from, is_variant, try_unwrap, unwrap` |
| `digest` | `0.10.7` | `registry+https://github.com/rust-lang/crates.io-index` | `9ed9a281f7bc9b7576e61468ba615a66a5c8cfdff42420a70aa82701a3b1e292` | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits | `block-buffer, core-api, default` |
| `dispatch2` | `0.3.1` | `registry+https://github.com/rust-lang/crates.io-index` | `1e0e367e4e7da84520dedcac1901e4da967309406d1e51017ae1abfb97adbd38` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 | `alloc, objc2` |
| `document-features` | `0.2.11` | `registry+https://github.com/rust-lang/crates.io-index` | `95249b50c6c185bee49034bcb378a49dc2b5dff0be90ff6616d31d64febab05d` | MIT OR Apache-2.0 | https://github.com/slint-ui/document-features | `default` |
| `either` | `1.15.0` | `registry+https://github.com/rust-lang/crates.io-index` | `48c757948c5ede0e46177b7add2e67155f70e33c07fea8284df6576da70b3719` | MIT OR Apache-2.0 | https://github.com/rayon-rs/either | `std, use_std` |
| `ena` | `0.14.4` | `registry+https://github.com/rust-lang/crates.io-index` | `eabffdaee24bd1bf95c5ef7cec31260444317e72ea56c4c91750e8b7ee58d5f1` | MIT OR Apache-2.0 | https://github.com/rust-lang/ena | `-` |
| `encoding_rs` | `0.8.35` | `registry+https://github.com/rust-lang/crates.io-index` | `75030f3c4f45dafd7586dd6780965a8c7e8e285a5ecb86713e63a79c5b2766f3` | (Apache-2.0 OR MIT) AND BSD-3-Clause | https://github.com/hsivonen/encoding_rs | `alloc, default` |
| `equivalent` | `1.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `877a4ace8713b0bcf2a4e7eec82529c029f1d0619886d18145fea96c3ffe5c0f` | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent | `-` |
| `euclid` | `0.22.14` | `registry+https://github.com/rust-lang/crates.io-index` | `f1a05365e3b1c6d1650318537c7460c6923f1abdd272ad6842baa2b509957a06` | MIT OR Apache-2.0 | https://github.com/servo/euclid | `std` |
| `fax` | `0.2.7` | `registry+https://github.com/rust-lang/crates.io-index` | `caf1079563223d5d59d83c85886a56e586cfd5c1a26292e971a0fa266531ac5a` | MIT | https://github.com/pdf-rs/fax | `-` |
| `fearless_simd` | `1.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `629c9d6b6f30b5604089e0980f4d924a013aacb1654a6cdf5d3d852224f68651` | Apache-2.0 OR MIT | https://github.com/linebender/fearless_simd | `std` |
| `fearless_simd_macros` | `0.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `3e91d94c6885390b81782d7ee60fd6583b33907333a8b340de25ccf468353315` | Apache-2.0 OR MIT | https://github.com/linebender/fearless_simd | `-` |
| `fixedbitset` | `0.5.9` | `registry+https://github.com/rust-lang/crates.io-index` | `5905ec755cc8386d7928353e4d356d2be3e89e81e5ca01fe630a1ca4d08dc175` | MIT OR Apache-2.0 | https://github.com/petgraph/fixedbitset | `-` |
| `flate2` | `1.1.11` | `registry+https://github.com/rust-lang/crates.io-index` | `626d4928326b646e70f65ed36fc7cf9f206bdb68a0019ee7c6b4709da4e084c4` | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs | `any_impl, default, miniz_oxide, runtime_detection, rust_backend` |
| `float_next_after` | `2.0.0` | `registry+https://github.com/rust-lang/crates.io-index` | `37007738a80ea34f969af54a3390dd72cacdef654974cfd449c9f6f72dbaac10` | MIT | https://gitlab.com/bronsonbdevost/next_afterf | `-` |
| `fnv` | `1.0.7` | `registry+https://github.com/rust-lang/crates.io-index` | `3f9eec918d3f24069decb9af1554cad7c880e2da24a9afd88aca000531ab82c1` | Apache-2.0 / MIT | https://github.com/servo/rust-fnv | `default, std` |
| `foldhash` | `0.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `77ce24cb58228fbb8aa041425bb1050850ac19177686ea6e0f41a70416f56fdb` | Zlib | https://github.com/orlp/foldhash | `std` |
| `font-types` | `0.12.7` | `registry+https://github.com/rust-lang/crates.io-index` | `835ae9d781464a79026cde387f9cbc650c9a4909d7cf7e658b127e3c35d2f35d` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations | `bytemuck, std` |
| `futures-core` | `0.3.31` | `registry+https://github.com/rust-lang/crates.io-index` | `05f29059c0c2090612e8d742178b0580d2dc940c837851ad723096f87af6663e` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | `alloc, std` |
| `futures-task` | `0.3.31` | `registry+https://github.com/rust-lang/crates.io-index` | `f90f7dce0722e95104fcb095585910c0977252f286e354b5e3bd38902cd99988` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | `alloc, std` |
| `futures-util` | `0.3.31` | `registry+https://github.com/rust-lang/crates.io-index` | `9fa08315bb612088cc391249efdc3bc77536f16c91f6cf495e6fbe85b20a4a81` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | `alloc, slab, std` |
| `geello` | `0.2.0` | `workspace` | `-` | MIT | https://github.com/DeadPoetSpoon/geello | `config-toml, default, read-geojson, read-geotiff, read-shapefile, render-cpu, render-gpu, rust-proj` |
| `generic-array` | `0.12.4` | `registry+https://github.com/rust-lang/crates.io-index` | `ffdf9f34f1447443d37393cc6c2b8313aebddcd96906caf34e54c68d8e57d7bd` | MIT | https://github.com/fizyk20/generic-array.git | `-` |
| `generic-array` | `0.13.3` | `registry+https://github.com/rust-lang/crates.io-index` | `f797e67af32588215eaaab8327027ee8e71b9dd0b2b26996aedf20c030fce309` | MIT | https://github.com/fizyk20/generic-array.git | `-` |
| `generic-array` | `0.14.7` | `registry+https://github.com/rust-lang/crates.io-index` | `85649ca51fd72272d7821adaf274ad91c288277713d9c18820d8499a7ff69e9a` | MIT | https://github.com/fizyk20/generic-array.git | `more_lengths` |
| `geo` | `0.33.1` | `registry+https://github.com/rust-lang/crates.io-index` | `30eb1fdc57c1e5cfd11826fe0caec4b9dc7901f3758263bb506228d88c8d9e9a` | MIT OR Apache-2.0 | https://github.com/georust/geo | `-` |
| `geo-types` | `0.7.20` | `registry+https://github.com/rust-lang/crates.io-index` | `777d18aa0f12f8b285331cd867133ee14422b3f023f6d388034c47d43e28786a` | MIT OR Apache-2.0 | https://github.com/georust/geo | `approx, default, rstar_0_12, serde, std` |
| `geographiclib-rs` | `0.2.5` | `registry+https://github.com/rust-lang/crates.io-index` | `f611040a2bb37eaa29a78a128d1e92a378a03e0b6e66ae27398d42b1ba9a7841` | MIT | https://github.com/georust/geographiclib-rs | `-` |
| `geojson` | `1.0.0` | `registry+https://github.com/rust-lang/crates.io-index` | `510c094bfc76ea34d02eee00833254945b70491d79a9c0b050abed6eaa799ffb` | MIT/Apache-2.0 | https://github.com/georust/geojson | `-` |
| `gimli` | `0.31.1` | `registry+https://github.com/rust-lang/crates.io-index` | `07e28edb80900c19c28f1072f2e8aeca7fa06b23cd4169cefe1af5aa3260783f` | MIT OR Apache-2.0 | https://github.com/gimli-rs/gimli | `read, read-core` |
| `glifo` | `0.5.0` | `registry+https://github.com/rust-lang/crates.io-index` | `9ae2e3b09ea1cc45c994a405e4d4154901d4caa867d3e1be3a02be641f62fefa` | Apache-2.0 OR MIT | https://github.com/linebender/vello | `std` |
| `gpu-allocator` | `0.28.0` | `registry+https://github.com/rust-lang/crates.io-index` | `51255ea7cfaadb6c5f1528d43e92a82acb2b96c43365989a28b2d44ee38f8795` | MIT OR Apache-2.0 | https://github.com/Traverse-Research/gpu-allocator | `d3d12, hashbrown, vulkan` |
| `guillotiere` | `0.7.0` | `registry+https://github.com/rust-lang/crates.io-index` | `6b17e70c989c36bad147b27a58d148c0741c51448aa5653436547323e524d0ab` | MIT/Apache-2.0 | https://github.com/nical/guillotiere | `-` |
| `half` | `2.6.0` | `registry+https://github.com/rust-lang/crates.io-index` | `459196ed295495a68f7d7fe1d84f6c4b7ff0e21fe3017b2f283c6fac3ad803c9` | MIT OR Apache-2.0 | https://github.com/VoidStarKat/half-rs | `alloc, default, num-traits, std` |
| `hash32` | `0.1.1` | `registry+https://github.com/rust-lang/crates.io-index` | `d4041af86e63ac4298ce40e5cca669066e75b6f1aa3390fe2561ffa5e1d9f4cc` | MIT OR Apache-2.0 | https://github.com/japaric/hash32 | `-` |
| `hash32` | `0.2.1` | `registry+https://github.com/rust-lang/crates.io-index` | `b0c35f58762feb77d74ebe43bdbc3210f09be9fe6742234d573bacc26ed92b67` | MIT OR Apache-2.0 | https://github.com/japaric/hash32 | `-` |
| `hash32` | `0.3.1` | `registry+https://github.com/rust-lang/crates.io-index` | `47d60b12902ba28e2730cd37e95b8c9223af2808df9e902d4df49588d1470606` | MIT OR Apache-2.0 | https://github.com/japaric/hash32 | `-` |
| `hashbrown` | `0.16.1` | `registry+https://github.com/rust-lang/crates.io-index` | `841d1cc9bed7f9236f321df977030373f4a4163ae1a7dbfe1a51a2c1a51d9100` | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown | `allocator-api2, default, default-hasher, equivalent, inline-more, raw-entry` |
| `hashbrown` | `0.17.1` | `registry+https://github.com/rust-lang/crates.io-index` | `ed5909b6e89a2db4456e54cd5f673791d7eca6732202bbf2a9cc504fe2f9b84a` | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown | `default-hasher, inline-more, raw-entry` |
| `heapless` | `0.6.1` | `registry+https://github.com/rust-lang/crates.io-index` | `634bd4d29cbf24424d0a4bfcbf80c6960129dc24424752a7d1d1390607023422` | MIT OR Apache-2.0 | https://github.com/japaric/heapless | `cas, default` |
| `heapless` | `0.7.17` | `registry+https://github.com/rust-lang/crates.io-index` | `cdc6457c0eb62c71aac4bc17216026d8410337c4126773b9c5daba343f17964f` | MIT OR Apache-2.0 | https://github.com/japaric/heapless | `atomic-polyfill, cas, default` |
| `heapless` | `0.8.0` | `registry+https://github.com/rust-lang/crates.io-index` | `0bfb9eb618601c89945a70e254898da93b13be0388091d42117462b265bb3fad` | MIT OR Apache-2.0 | https://github.com/rust-embedded/heapless | `-` |
| `i_float` | `1.16.0` | `registry+https://github.com/rust-lang/crates.io-index` | `813145bb0ad5b60f55cbbf3c74cdceda1c0a9d253b35c4cc36ae0df7887cb78f` | MIT | https://github.com/iShape-Rust/iFloat | `core, default, float_pt` |
| `i_key_sort` | `0.10.3` | `registry+https://github.com/rust-lang/crates.io-index` | `d73d122b937fca067feb0ad74f62388920272b27c356d4df2d0cfdd59e044cf0` | MIT | https://github.com/iShape-Rust/iKeySort | `default` |
| `i_overlay` | `4.5.2` | `registry+https://github.com/rust-lang/crates.io-index` | `8dd314b4668e2b3a12508f2e125558c82a6c0a8636fa5107a900f79ce414e450` | MIT OR Apache-2.0 | https://github.com/iShape-Rust/iOverlay | `-` |
| `i_shape` | `1.18.0` | `registry+https://github.com/rust-lang/crates.io-index` | `bfa9eac533d7509a8ab87672b60ac610c17240f9ea4851d26227689fdfe349c8` | MIT | https://github.com/iShape-Rust/iShape | `-` |
| `i_tree` | `0.18.0` | `registry+https://github.com/rust-lang/crates.io-index` | `4804bdc1dc124eb7e1aa9e144ecc04096bcf787a10a15fa44af682b51f0f6cce` | MIT | https://github.com/iShape-Rust/iTree | `-` |
| `indexmap` | `2.14.2` | `registry+https://github.com/rust-lang/crates.io-index` | `cc4e190f5d26ca7051642629da2c52fc03bde85a03197c99408dcd291734c855` | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap | `default, std` |
| `itertools` | `0.14.0` | `registry+https://github.com/rust-lang/crates.io-index` | `2b192c782037fadd9cfa75548310488aabdbf3d2da73885b31bd0abd03351285` | MIT OR Apache-2.0 | https://github.com/rust-itertools/itertools | `default, use_alloc, use_std` |
| `itoa` | `1.0.15` | `registry+https://github.com/rust-lang/crates.io-index` | `4a5f13b858c8d314ee3e8f639011f7ccefe71f97f96e50151fb991f267928e2c` | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa | `-` |
| `js-sys` | `0.3.107` | `registry+https://github.com/rust-lang/crates.io-index` | `8dc09d2154098bbf4ef54eab6b117a3c24cbbffe94046b16ee6d6d4510584d3a` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys | `default, std, unsafe-eval` |
| `keccak` | `0.1.6` | `registry+https://github.com/rust-lang/crates.io-index` | `cb26cec98cce3a3d96cbb7bced3c4b16e3d13f27ec56dbd62cbc8f39cfb9d653` | Apache-2.0 OR MIT | https://github.com/RustCrypto/sponges/tree/master/keccak | `-` |
| `kurbo` | `0.13.1` | `registry+https://github.com/rust-lang/crates.io-index` | `4b60dfc32f652b926df6192e55525b16d186c69d47876c3ead4da5cc9f8450e2` | Apache-2.0 OR MIT | https://github.com/linebender/kurbo | `std` |
| `lalrpop` | `0.22.2` | `registry+https://github.com/rust-lang/crates.io-index` | `ba4ebbd48ce411c1d10fb35185f5a51a7bfa3d8b24b4e330d30c9e3a34129501` | Apache-2.0 OR MIT | https://github.com/lalrpop/lalrpop | `-` |
| `lalrpop-util` | `0.22.2` | `registry+https://github.com/rust-lang/crates.io-index` | `b5baa5e9ff84f1aefd264e6869907646538a52147a755d494517a8007fb48733` | Apache-2.0 OR MIT | https://github.com/lalrpop/lalrpop | `default, regex-automata, std, unicode` |
| `lexical` | `7.0.5` | `registry+https://github.com/rust-lang/crates.io-index` | `1bc8a009b2ff1f419ccc62706f04fe0ca6e67b37460513964a3dfdb919bb37d6` | MIT/Apache-2.0 | https://github.com/Alexhuszagh/rust-lexical | `default, format, parse-floats, parse-integers, power-of-two, std, write-floats, write-integers` |
| `lexical-core` | `1.0.6` | `registry+https://github.com/rust-lang/crates.io-index` | `7d8d125a277f807e55a77304455eb7b1cb52f2b18c143b60e766c120bd64a594` | MIT/Apache-2.0 | https://github.com/Alexhuszagh/rust-lexical | `format, lexical-parse-float, lexical-parse-integer, lexical-write-float, lexical-write-integer, parse-floats, parse-integers, power-of-two, std, write-floats, write-integers` |
| `lexical-parse-float` | `1.0.6` | `registry+https://github.com/rust-lang/crates.io-index` | `52a9f232fbd6f550bc0137dcb5f99ab674071ac2d690ac69704593cb4abbea56` | MIT/Apache-2.0 | https://github.com/Alexhuszagh/rust-lexical | `format, power-of-two, std` |
| `lexical-parse-integer` | `1.0.6` | `registry+https://github.com/rust-lang/crates.io-index` | `9a7a039f8fb9c19c996cd7b2fcce303c1b2874fe1aca544edc85c4a5f8489b34` | MIT/Apache-2.0 | https://github.com/Alexhuszagh/rust-lexical | `format, power-of-two, std` |
| `lexical-util` | `1.0.7` | `registry+https://github.com/rust-lang/crates.io-index` | `2604dd126bb14f13fb5d1bd6a66155079cb9fa655b37f875b3a742c705dbed17` | MIT/Apache-2.0 | https://github.com/Alexhuszagh/rust-lexical | `format, parse-floats, parse-integers, power-of-two, std, write-floats, write-integers` |
| `lexical-write-float` | `1.0.6` | `registry+https://github.com/rust-lang/crates.io-index` | `50c438c87c013188d415fbabbb1dceb44249ab81664efbd31b14ae55dabb6361` | MIT/Apache-2.0 | https://github.com/Alexhuszagh/rust-lexical | `format, power-of-two, std` |
| `lexical-write-integer` | `1.0.6` | `registry+https://github.com/rust-lang/crates.io-index` | `409851a618475d2d5796377cad353802345cba92c867d9fbcde9cf4eac4e14df` | MIT/Apache-2.0 | https://github.com/Alexhuszagh/rust-lexical | `format, power-of-two, std` |
| `libc` | `0.2.191` | `registry+https://github.com/rust-lang/crates.io-index` | `65cd87c9a6d60536580b316ad985726104730a5151c99845f2490428ef91240a` | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | `default, std` |
| `libloading` | `0.8.8` | `registry+https://github.com/rust-lang/crates.io-index` | `07033963ba89ebaf1584d767badaa2e8fcec21aedea6b8c0346d487d49c28667` | ISC | https://github.com/nagisa/rust_libloading/ | `-` |
| `libm` | `0.2.15` | `registry+https://github.com/rust-lang/crates.io-index` | `f9fbbcab51052fe104eb5e5d351cf728d30a5be1fe14d9be8a3b097481fb97de` | MIT | https://github.com/rust-lang/compiler-builtins | `arch, default` |
| `linebender_resource_handle` | `0.1.1` | `registry+https://github.com/rust-lang/crates.io-index` | `d4a5ff6bcca6c4867b1c4fd4ef63e4db7436ef363e0ad7531d1558856bae64f4` | Apache-2.0 OR MIT | https://github.com/linebender/raw_resource_handle | `std` |
| `litrs` | `0.4.1` | `registry+https://github.com/rust-lang/crates.io-index` | `b4ce301924b7887e9d637144fdade93f9dfff9b60981d4ac161db09720d39aa5` | MIT/Apache-2.0 | https://github.com/LukasKalbertodt/litrs/ | `-` |
| `lock_api` | `0.4.13` | `registry+https://github.com/rust-lang/crates.io-index` | `96936507f153605bddfcda068dd804796c84324ed2510809e5b2a624c81da765` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot | `atomic_usize, default` |
| `log` | `0.4.35` | `registry+https://github.com/rust-lang/crates.io-index` | `5d15f86d12da44cd8d4ec3c73984537eb7f4cadcb9776a575a0a8a05ca11005c` | MIT OR Apache-2.0 | https://github.com/rust-lang/log | `-` |
| `logos` | `0.16.1` | `registry+https://github.com/rust-lang/crates.io-index` | `eb2c55a318a87600ea870ff8c2012148b44bf18b74fad48d0f835c38c7d07c5f` | MIT OR Apache-2.0 | https://github.com/maciejhirsz/logos | `default, export_derive, logos-derive, std` |
| `logos-codegen` | `0.16.1` | `registry+https://github.com/rust-lang/crates.io-index` | `58b3ffaa284e1350d017a57d04ada118c4583cf260c8fb01e0fe28a2e9cf8970` | MIT OR Apache-2.0 | https://github.com/maciejhirsz/logos | `-` |
| `logos-derive` | `0.16.1` | `registry+https://github.com/rust-lang/crates.io-index` | `52d3a9855747c17eaf4383823f135220716ab49bea5fbea7dd42cc9a92f8aa31` | MIT OR Apache-2.0 | https://github.com/maciejhirsz/logos | `-` |
| `memchr` | `2.8.3` | `registry+https://github.com/rust-lang/crates.io-index` | `cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98` | Unlicense OR MIT | https://github.com/BurntSushi/memchr | `alloc, std` |
| `miniz_oxide` | `0.8.9` | `registry+https://github.com/rust-lang/crates.io-index` | `1fa76a2c86f704bdb222d66965fb3d63269ce38518b83cb0575fca855ebb6316` | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide | `-` |
| `miniz_oxide` | `0.9.1` | `registry+https://github.com/rust-lang/crates.io-index` | `b63fbc4a50860e98e7b2aa7804ded1db5cbc3aff9193adaff57a6931bf7c4b4c` | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide | `default, simd, simd-adler32, with-alloc` |
| `naga` | `30.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `fd0dd362bf26c0ffdd143df6da66a25d287e196eff482f117771010b207ad163` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `default, glsl-out, hlsl-out, spv-out, wgsl-in, wgsl-out` |
| `naga-types` | `30.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `14474170feb673cfb18b3565111a9869797cc1d290fb9ec969c53f5e1e231c6d` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `std` |
| `new_debug_unreachable` | `1.0.6` | `registry+https://github.com/rust-lang/crates.io-index` | `650eef8c711430f1a879fdd01d4745a7deea475becfb90269c06775983bbf086` | MIT | https://github.com/mbrubeck/rust-debug-unreachable | `-` |
| `nom` | `8.0.0` | `registry+https://github.com/rust-lang/crates.io-index` | `df9761775871bdef83bee530e60050f7e54b1105350d6884eb0fb4f46c2f9405` | MIT | https://github.com/rust-bakery/nom | `alloc, default, std` |
| `num-conv` | `0.2.2` | `registry+https://github.com/rust-lang/crates.io-index` | `521739c6d2bac4aa25192232afe6841231376b2b26d4d9fae5ecf8ca5772e441` | MIT OR Apache-2.0 | https://github.com/jhpratt/num-conv | `-` |
| `num-traits` | `0.2.19` | `registry+https://github.com/rust-lang/crates.io-index` | `071dfc062690e90b734c0b2273ce72ad0ffa95f0c74596bc250dcfd960262841` | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits | `default, libm, std` |
| `objc2` | `0.6.5` | `registry+https://github.com/rust-lang/crates.io-index` | `08849bbd4767dfae9457696856ae1c84fe4e0281bbe4a7abff2d0e06fb7981f8` | MIT | https://github.com/madsmtm/objc2 | `alloc, default, std` |
| `objc2-core-foundation` | `0.3.2` | `registry+https://github.com/rust-lang/crates.io-index` | `2a180dd8642fa45cdb7dd721cd4c11b1cadd4929ce112ebd8b9f5803cc79d536` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 | `CFArray, CFAttributedString, CFCGTypes, CFCalendar, CFCharacterSet, CFData, CFDate, CFDictionary, CFError, CFFileSecurity, CFLocale, CFMachPort, CFMessagePort, CFRunLoop, CFSet, CFStream, CFURL, bitflags, objc2` |
| `objc2-encode` | `4.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `ef25abbcd74fb2609453eb695bd2f860d389e457f67dc17cafc8b8cbc89d0c33` | MIT | https://github.com/madsmtm/objc2 | `alloc, std` |
| `objc2-foundation` | `0.3.2` | `registry+https://github.com/rust-lang/crates.io-index` | `e3e0adef53c21f888deb4fa59fc59f7eb17404926ee8a6f59f5df0fd7f9f3272` | MIT | https://github.com/madsmtm/objc2 | `NSArray, NSBundle, NSDictionary, NSError, NSGeometry, NSKeyValueObserving, NSNull, NSObjCRuntime, NSObject, NSRange, NSString, NSThread, NSURL, NSValue, alloc, bitflags, objc2-core-foundation` |
| `objc2-metal` | `0.3.2` | `registry+https://github.com/rust-lang/crates.io-index` | `a0125f776a10d00af4152d74616409f0d4a2053a6f57fa5b7d6aa2854ac04794` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 | `MTLAllocation, MTLDevice, MTLDrawable, MTLGPUAddress, MTLLibrary, MTLPixelFormat, MTLResidencySet, MTLResource, MTLTexture, bitflags` |
| `objc2-quartz-core` | `0.3.2` | `registry+https://github.com/rust-lang/crates.io-index` | `96c1358452b371bf9f104e21ec536d37a650eb10f7ee379fff67d2e08d537f1f` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 | `CALayer, CAMetalLayer, bitflags, objc2-core-foundation, objc2-metal` |
| `object` | `0.36.7` | `registry+https://github.com/rust-lang/crates.io-index` | `62948e14d923ea95ea2c7c86c71013138b66525b86bdc08d2dcc262bdb497b87` | Apache-2.0 OR MIT | https://github.com/gimli-rs/object | `archive, coff, elf, macho, pe, read_core, unaligned, xcoff` |
| `once_cell` | `1.21.4` | `registry+https://github.com/rust-lang/crates.io-index` | `9f7c3e4beb33f85d45ae3e3a1792185706c8e16d043238c593331cc7cd313b50` | MIT OR Apache-2.0 | https://github.com/matklad/once_cell | `alloc, race, std` |
| `ordered-channel` | `1.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `95be4d57809897b5a7539fc15a7dfe0e84141bc3dfaa2e9b1b27caa90acf61ab` | MIT OR Apache-2.0 | https://gitlab.com/kornelski/ordered-channel | `crossbeam-channel` |
| `ordered-float` | `4.6.0` | `registry+https://github.com/rust-lang/crates.io-index` | `7bb71e1b3fa6ca1c61f383464aaf2bb0e2f8e772a1f01d486832464de363b951` | MIT | https://github.com/reem/rust-ordered-float | `-` |
| `parking_lot` | `0.12.4` | `registry+https://github.com/rust-lang/crates.io-index` | `70d58bf43669b5795d1576d0641cfb6fbb2057bf629506267a92807158584a13` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot | `default` |
| `parking_lot_core` | `0.9.11` | `registry+https://github.com/rust-lang/crates.io-index` | `bc838d2a56b5b1a6c25f55575dfc605fabb63bb2365f6c2353ef9159aa69e4a5` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot | `-` |
| `pdqselect` | `0.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `4ec91767ecc0a0bbe558ce8c9da33c068066c57ecc8bb8477ef8c1ad3ef77c27` | Apache-2.0/MIT | - | `-` |
| `peniko` | `0.6.1` | `registry+https://github.com/rust-lang/crates.io-index` | `839c8299360d2e998bdb106dc0a6cd71dcc5f4df51df1b620361bf50e283cca6` | Apache-2.0 OR MIT | https://github.com/linebender/peniko | `bytemuck, std` |
| `petgraph` | `0.7.1` | `registry+https://github.com/rust-lang/crates.io-index` | `3672b37090dbd86368a4145bc067582552b29c27377cad4e0a306c97f9bd7772` | MIT OR Apache-2.0 | https://github.com/petgraph/petgraph | `-` |
| `phf_shared` | `0.11.3` | `registry+https://github.com/rust-lang/crates.io-index` | `67eabc2ef2a60eb7faa00097bd1ffdb5bd28e62bf39990626a582201b7a754e5` | MIT | https://github.com/rust-phf/rust-phf | `default, std` |
| `pin-project-lite` | `0.2.16` | `registry+https://github.com/rust-lang/crates.io-index` | `3b3cff922bd51709b605d9ead9aa71031d81447142d828eb4a6eba76fe619f9b` | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite | `-` |
| `pin-utils` | `0.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `8b870d8c151b6f2fb93e84a13146138f05d02ed11c7e7c54f8826aaaf7c9f184` | MIT OR Apache-2.0 | https://github.com/rust-lang-nursery/pin-utils | `-` |
| `polycool` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `50596ddc09eb5ad5f75cacd40209568e66df71baf86e1499a0e99c4cff12a5a6` | MIT OR Apache-2.0 | https://github.com/linebender/kurbo | `std` |
| `portable-atomic` | `1.15.0` | `registry+https://github.com/rust-lang/crates.io-index` | `05c8b63e8d9609db387f0324918f81d68fe27748f084ef092fb35954d0539a85` | Apache-2.0 OR MIT | https://github.com/taiki-e/portable-atomic | `default, fallback, require-cas` |
| `portable-atomic-util` | `0.2.8` | `registry+https://github.com/rust-lang/crates.io-index` | `10ab3eb7f3becc3a1cbc4f2c6f20267996cfc1a6467a873763411b136a122715` | Apache-2.0 OR MIT | https://github.com/taiki-e/portable-atomic-util | `alloc, default` |
| `powerfmt` | `0.2.1` | `registry+https://github.com/rust-lang/crates.io-index` | `4a6394b9e965e73d0a289ee54f589087e2c676aedf60885baf52c76b771e4958` | MIT OR Apache-2.0 | https://github.com/jhpratt/powerfmt | `-` |
| `precomputed-hash` | `0.1.1` | `registry+https://github.com/rust-lang/crates.io-index` | `925383efa346730478fb4838dbe9137d2a47675ad789c546d150a6e1dd4ab31c` | MIT | https://github.com/emilio/precomputed-hash | `-` |
| `presser` | `0.3.1` | `registry+https://github.com/rust-lang/crates.io-index` | `e8cf8e6a8aa66ce33f63993ffc4ea4271eb5b0530a9002db8455ea6050c77bfa` | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/presser | `-` |
| `proc-macro2` | `1.0.95` | `registry+https://github.com/rust-lang/crates.io-index` | `02b3e5e68a3a1a02aad3ec490a98007cbc13c37cbe84a3cd7b8e406d76e7f778` | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 | `default, proc-macro` |
| `profiling` | `1.0.16` | `registry+https://github.com/rust-lang/crates.io-index` | `afbdc74edc00b6f6a218ca6a5364d6226a259d4b8ea1af4a0ea063f27e179f4d` | MIT OR Apache-2.0 | https://github.com/aclysma/profiling | `-` |
| `proj4rs` | `0.3.0` | `registry+https://github.com/rust-lang/crates.io-index` | `c237a4e4da4e0f2eae7a5d078df926894ccf1b315043dbccebc413888487d562` | MIT OR Apache-2.0 | https://github.com/3liz/proj4rs/ | `-` |
| `proj4rs-geodesic` | `0.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `bdb2ff6e4d7329dc428ca2cb40f4c1ea85ef6c10aa6bd410294a1c835cd55567` | MIT OR Apache-2.0 | https://github.com/3liz/proj4rs/ | `-` |
| `proj4wkt` | `0.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `58c81c82adeaefed1216edce88aae980cba2c1eff3e8be8fc0f405b04765f82a` | MIT OR Apache-2.0 | https://github.com/3liz/proj4wkt-rs/ | `-` |
| `quick-error` | `2.0.1` | `registry+https://github.com/rust-lang/crates.io-index` | `a993555f31e5a609f617c12db6250dedcac1b0a85076912c436e6fc9b2c8e6a3` | MIT/Apache-2.0 | http://github.com/tailhook/quick-error | `-` |
| `quote` | `1.0.40` | `registry+https://github.com/rust-lang/crates.io-index` | `1885c039570dc00dcb4ff087a89e185fd56bae234ddc7f056a945bf36467248d` | MIT OR Apache-2.0 | https://github.com/dtolnay/quote | `default, proc-macro` |
| `rand` | `0.10.3` | `registry+https://github.com/rust-lang/crates.io-index` | `65c9fb96cbc91e3478eaae79a69fcd3f1ae4ad052e471fe6732fff548984b4af` | MIT OR Apache-2.0 | https://github.com/rust-random/rand | `alloc` |
| `rand_core` | `0.10.1` | `registry+https://github.com/rust-lang/crates.io-index` | `63b8176103e19a2643978565ca18b50549f6101881c443590420e4dc998a3c69` | MIT OR Apache-2.0 | https://github.com/rust-random/rand_core | `-` |
| `rand_pcg` | `0.10.2` | `registry+https://github.com/rust-lang/crates.io-index` | `caa0f4137e1c0a72f4c651489402276c8e8e1cf081f3b0ba156d2cbeef09e86a` | MIT OR Apache-2.0 | https://github.com/rust-random/rngs | `-` |
| `range-alloc` | `0.1.4` | `registry+https://github.com/rust-lang/crates.io-index` | `c3d6831663a5098ea164f89cff59c6284e95f4e3c76ce9848d4529f5ccca9bde` | MIT OR Apache-2.0 | https://github.com/gfx-rs/range-alloc | `-` |
| `raw-window-handle` | `0.6.2` | `registry+https://github.com/rust-lang/crates.io-index` | `20675572f6f24e9e76ef639bc5552774ed45f1c30e2951e1e99c59888861c539` | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/raw-window-handle | `alloc, std` |
| `raw-window-metal` | `1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `40d213455a5f1dc59214213c7330e074ddf8114c9a42411eb890c767357ce135` | MIT OR Apache-2.0 | https://github.com/rust-windowing/raw-window-metal | `alloc, default, std` |
| `rayon` | `1.12.0` | `registry+https://github.com/rust-lang/crates.io-index` | `fb39b166781f92d482534ef4b4b1b2568f42613b53e5b6c160e24cfbfa30926d` | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon | `-` |
| `rayon-core` | `1.13.0` | `registry+https://github.com/rust-lang/crates.io-index` | `22e18b0f0062d30d4230b2e85ff77fdfe4326feb054b9783a3460d8435c8ab91` | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon | `-` |
| `read-fonts` | `0.46.0` | `registry+https://github.com/rust-lang/crates.io-index` | `bdaa4fffe9ff3ca2c353fa9729792805b9b3e324695ff5a081f860612e81b330` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations | `std` |
| `redox_syscall` | `0.5.13` | `registry+https://github.com/rust-lang/crates.io-index` | `0d04b7d0ee6b4a0207a0a7adb104d23ecb0b47d6beae7152d0fa34b692b29fd6` | MIT | https://gitlab.redox-os.org/redox-os/syscall | `default, userspace` |
| `regex` | `1.11.1` | `registry+https://github.com/rust-lang/crates.io-index` | `b544ef1b4eac5dc2db33ea63606ae9ffcfac26c1416a2806ae0bf5f56b201191` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex | `std` |
| `regex-automata` | `0.4.9` | `registry+https://github.com/rust-lang/crates.io-index` | `809e8dc61f6de73b46c85f4c96486310fe304c434cfa43669d7b40f711150908` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex/tree/master/regex-automata | `alloc, default, dfa, dfa-build, dfa-onepass, dfa-search, hybrid, meta, nfa, nfa-backtrack, nfa-pikevm, nfa-thompson, perf, perf-inline, perf-literal, perf-literal-multisubstring, perf-literal-substring, std, syntax, unicode, unicode-age, unicode-bool, unicode-case, unicode-gencat, unicode-perl, unicode-script, unicode-segment, unicode-word-boundary` |
| `regex-syntax` | `0.8.5` | `registry+https://github.com/rust-lang/crates.io-index` | `2b15c43186be67a4fd63bee50d0303afffcef381492ebe2c5d87f324e1b8815c` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex/tree/master/regex-syntax | `default, std, unicode, unicode-age, unicode-bool, unicode-case, unicode-gencat, unicode-perl, unicode-script, unicode-segment` |
| `renderdoc-sys` | `1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `19b30a45b0cd0bcca8037f3d0dc3421eaf95327a17cad11964fb8179b4fc4832` | MIT OR Apache-2.0 | https://github.com/ebkalderon/renderdoc-rs | `-` |
| `robust` | `1.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `4e27ee8bb91ca0adcf0ecb116293afa12d393f9c2b9b9cd54d33e8078fe19839` | MIT OR Apache-2.0 | https://github.com/georust/robust | `-` |
| `rstar` | `0.10.0` | `registry+https://github.com/rust-lang/crates.io-index` | `1f39465655a1e3d8ae79c6d9e007f4953bfc5d55297602df9dc38f9ae9f1359a` | MIT OR Apache-2.0 | https://github.com/georust/rstar | `default, serde` |
| `rstar` | `0.11.0` | `registry+https://github.com/rust-lang/crates.io-index` | `73111312eb7a2287d229f06c00ff35b51ddee180f017ab6dec1f69d62ac098d6` | MIT OR Apache-2.0 | https://github.com/georust/rstar | `default, serde` |
| `rstar` | `0.12.2` | `registry+https://github.com/rust-lang/crates.io-index` | `421400d13ccfd26dfa5858199c30a5d76f9c54e0dba7575273025b43c5175dbb` | MIT OR Apache-2.0 | https://github.com/georust/rstar | `default, serde` |
| `rstar` | `0.13.1` | `registry+https://github.com/rust-lang/crates.io-index` | `90e7279515baf3838062fa9c495a408bc2f3abbf9cd2ac8c2d4108b90fa29b89` | MIT OR Apache-2.0 | https://github.com/georust/rstar | `default, serde` |
| `rstar` | `0.8.4` | `registry+https://github.com/rust-lang/crates.io-index` | `3a45c0e8804d37e4d97e55c6f258bc9ad9c5ee7b07437009dd152d764949a27c` | MIT/Apache-2.0 | https://github.com/Stoeoef/rstar | `default, serde` |
| `rstar` | `0.9.3` | `registry+https://github.com/rust-lang/crates.io-index` | `b40f1bfe5acdab44bc63e6699c28b74f75ec43afb59f3eda01e145aff86a25fa` | MIT/Apache-2.0 | https://github.com/georust/rstar | `default, serde` |
| `rustc-demangle` | `0.1.25` | `registry+https://github.com/rust-lang/crates.io-index` | `989e6739f80c4ad5b13e0fd7fe89531180375b18520cc8c82080e4dc4035b84f` | MIT/Apache-2.0 | https://github.com/rust-lang/rustc-demangle | `-` |
| `rustc-hash` | `1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `08d43f7aa6b08d49f382cde6a7982047c3426db949b1424bc4b7ec9ae12c6ce2` | Apache-2.0/MIT | https://github.com/rust-lang-nursery/rustc-hash | `-` |
| `rustc_version` | `0.4.1` | `registry+https://github.com/rust-lang/crates.io-index` | `cfcb3a22ef46e85b45de6ee7e79d063319ebb6594faafcf1c225ea92ab6e9b92` | MIT OR Apache-2.0 | https://github.com/djc/rustc-version-rs | `-` |
| `rustversion` | `1.0.21` | `registry+https://github.com/rust-lang/crates.io-index` | `8a0d197bd2c9dc6e53b84da9556a69ba4cdfab8619eb41a8bd1cc2027a0f6b1d` | MIT OR Apache-2.0 | https://github.com/dtolnay/rustversion | `-` |
| `same-file` | `1.0.6` | `registry+https://github.com/rust-lang/crates.io-index` | `93fc1dc3aaa9bfed95e02e6eadabb4baf7e3078b0bd1b4d7b6b0b68378900502` | Unlicense/MIT | https://github.com/BurntSushi/same-file | `-` |
| `scopeguard` | `1.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `94143f37725109f92c262ed2cf5e59bce7498c01bcc1502d7b9afe439a4e9f49` | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard | `-` |
| `semver` | `1.0.28` | `registry+https://github.com/rust-lang/crates.io-index` | `8a7852d02fc848982e0c167ef163aaff9cd91dc640ba85e263cb1ce46fae51cd` | MIT OR Apache-2.0 | https://github.com/dtolnay/semver | `default, std` |
| `serde` | `1.0.229` | `registry+https://github.com/rust-lang/crates.io-index` | `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | `alloc, default, derive, serde_derive, std` |
| `serde_core` | `1.0.229` | `registry+https://github.com/rust-lang/crates.io-index` | `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | `alloc, result, std` |
| `serde_derive` | `1.0.229` | `registry+https://github.com/rust-lang/crates.io-index` | `e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | `default` |
| `serde_json` | `1.0.154` | `registry+https://github.com/rust-lang/crates.io-index` | `e7e9cc8b1b85264074fbcc02a88680c4096b1e47df8f739dceb03bf482f04bd6` | MIT OR Apache-2.0 | https://github.com/serde-rs/json | `default, std` |
| `serde_spanned` | `1.1.3` | `registry+https://github.com/rust-lang/crates.io-index` | `9c187e0a369471c550af028cba56d4f4fa2a489ad28151ae95a4d340a2a8b782` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | `alloc, serde` |
| `sha3` | `0.10.9` | `registry+https://github.com/rust-lang/crates.io-index` | `77fd7028345d415a4034cf8777cd4f8ab1851274233b45f84e3d955502d93874` | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | `-` |
| `shapefile` | `0.9.0` | `registry+https://github.com/rust-lang/crates.io-index` | `92fedc0c44dc53a02655cd8dccd2fdf3159c372c0f6eb59115e889f11325e4f0` | MIT | https://github.com/tmontaigu/shapefile-rs | `encoding_rs, geo-types` |
| `sif-itree` | `0.4.1` | `registry+https://github.com/rust-lang/crates.io-index` | `d7f45b8998ced5134fb1d75732c77842a3e888f19c1ff98481822e8fbfbf930b` | MIT OR Apache-2.0 | https://codeberg.org/adamreichold/sif-itree | `-` |
| `simd-adler32` | `0.3.10` | `registry+https://github.com/rust-lang/crates.io-index` | `3a219298ac11a56ea9a6d2120044824d6f01aeb034955e7af7bc16858527deea` | MIT | https://github.com/mcountryman/simd-adler32 | `-` |
| `siphasher` | `1.0.1` | `registry+https://github.com/rust-lang/crates.io-index` | `56199f7ddabf13fe5074ce809e7d3f42b42ae711800501b5b16ea82ad029c39d` | MIT/Apache-2.0 | https://github.com/jedisct1/rust-siphash | `default, std` |
| `skrifa` | `0.49.0` | `registry+https://github.com/rust-lang/crates.io-index` | `a7459fb60888ad9a5f367dd293fa2fb9df142cb451aa6bfd6cd66a7d6a9b739d` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations | `autohint_shaping, std` |
| `slab` | `0.4.10` | `registry+https://github.com/rust-lang/crates.io-index` | `04dc19736151f35336d325007ac991178d504a119863a2fcb3758cdb5e52c50d` | MIT | https://github.com/tokio-rs/slab | `default, std` |
| `smallvec` | `1.15.1` | `registry+https://github.com/rust-lang/crates.io-index` | `67b1b7a3b5fe4f1376887184045fcf45c69e92af734b7aaddc05fb777b6fbd03` | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec | `const_generics, const_new, union` |
| `spin` | `0.9.8` | `registry+https://github.com/rust-lang/crates.io-index` | `6980e8d7511241f8acf4aebddbb1ff938df5eebe98691418c4468d0b72a96a67` | MIT | https://github.com/mvdnes/spin-rs.git | `barrier, default, lazy, lock_api, lock_api_crate, mutex, once, rwlock, spin_mutex` |
| `spirv` | `0.4.0+sdk-1.4.341.0` | `registry+https://github.com/rust-lang/crates.io-index` | `d9571ea910ebd84c86af4b3ed27f9dbdc6ad06f17c5f96146b2b671e2976744f` | Apache-2.0 | https://github.com/gfx-rs/rspirv | `-` |
| `stable_deref_trait` | `1.2.0` | `registry+https://github.com/rust-lang/crates.io-index` | `a8f112729512f8e442d81f95a8a7ddf2b7c6b8a1a6f509a95864142b30cab2d3` | MIT/Apache-2.0 | https://github.com/storyyeller/stable_deref_trait | `alloc` |
| `static_assertions` | `1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `a2eb9349b6444b326872e140eb1cf5e7c522154d69e7a0ffb0fb81c06b37543f` | MIT OR Apache-2.0 | https://github.com/nvzqz/static-assertions-rs | `-` |
| `string_cache` | `0.8.9` | `registry+https://github.com/rust-lang/crates.io-index` | `bf776ba3fa74f83bf4b63c3dcbbf82173db2632ed8452cb2d891d33f459de70f` | MIT OR Apache-2.0 | https://github.com/servo/string-cache | `-` |
| `syn` | `2.0.103` | `registry+https://github.com/rust-lang/crates.io-index` | `e4307e30089d6fd6aff212f2da3a1f9e32f3223b1f010fb09b7c95f90f3ca1e8` | MIT OR Apache-2.0 | https://github.com/dtolnay/syn | `clone-impls, default, derive, extra-traits, fold, full, parsing, printing, proc-macro, visit` |
| `syn` | `3.0.10` | `registry+https://github.com/rust-lang/crates.io-index` | `4ae9ccbea3a7b0b09443036490c8f34a8b1b760fd345e5c0dae0aac718faea79` | MIT OR Apache-2.0 | https://github.com/dtolnay/syn | `clone-impls, default, derive, extra-traits, fold, full, parsing, printing, proc-macro, visit, visit-mut` |
| `synstructure` | `0.13.2` | `registry+https://github.com/rust-lang/crates.io-index` | `728a70f3dbaf5bab7f0c4b1ac8d7ae5ea60a4b5549c8a5914361c99147a709d2` | MIT | https://github.com/mystor/synstructure | `default, proc-macro` |
| `term` | `1.2.1` | `registry+https://github.com/rust-lang/crates.io-index` | `d8c27177b12a6399ffc08b98f76f7c9a1f4fe9fc967c784c5a071fa8d93cf7e1` | MIT OR Apache-2.0 | https://github.com/Stebalien/term | `default` |
| `thiserror` | `2.0.21` | `registry+https://github.com/rust-lang/crates.io-index` | `09e52cb86a36cede5cb101bf8908837b3e4c6e5e59fe7fd85c23fb56200d189e` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror | `default, std` |
| `thiserror-impl` | `2.0.21` | `registry+https://github.com/rust-lang/crates.io-index` | `fe5197923287db20a58125f0bc85c062f7f2c892de97b18c356f9efb14b28524` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror | `-` |
| `thread_local` | `1.1.10` | `registry+https://github.com/rust-lang/crates.io-index` | `1ad99c4c6d32803332c548b1af0540b357b3f5fc0be8f6c6bfe8b2e6ae784070` | MIT OR Apache-2.0 | https://github.com/Amanieu/thread_local-rs | `-` |
| `tiff` | `0.11.3` | `registry+https://github.com/rust-lang/crates.io-index` | `b63feaf3343d35b6ca4d50483f94843803b0f51634937cc2ec519fc32232bc52` | MIT | https://github.com/image-rs/image-tiff | `deflate, fax, jpeg, lzw` |
| `time` | `0.3.55` | `registry+https://github.com/rust-lang/crates.io-index` | `cdb87b95ec50ddfa440816d227a17b2ccbdda963a316a727fda0fc4334f7d134` | MIT OR Apache-2.0 | https://github.com/time-rs/time | `alloc, default, std` |
| `time-core` | `0.1.9` | `registry+https://github.com/rust-lang/crates.io-index` | `9e1c906769ad99c88eaa54e728060edef082f8e358ff32030cb7c7d315e81109` | MIT OR Apache-2.0 | https://github.com/time-rs/time | `-` |
| `tinyvec` | `1.13.3` | `registry+https://github.com/rust-lang/crates.io-index` | `fd3ca314f692efd6c868f8408f53fe444634a845f96c028b97d35f6a1f79f0ee` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/tinyvec | `alloc, default, serde` |
| `tokio` | `1.45.1` | `registry+https://github.com/rust-lang/crates.io-index` | `75ef51a33ef1da925cea3e4eb122833cb377c61439ca401b770f54902b806779` | MIT | https://github.com/tokio-rs/tokio | `rt, time` |
| `toml` | `1.2.1+spec-1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `d5755c51863cdac711f1956baf8eb9588e060755f7d3c4c979e70ea1e1f32cd7` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | `display, parse, serde` |
| `toml_datetime` | `1.1.3+spec-1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `173f25c883cd2176a2d8212155638c0a60d484a9d84546d270866f40f56b6e49` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | `alloc, serde` |
| `toml_parser` | `1.1.5+spec-1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `baa693a8032d7e1cada7d0041e96126df243179ff061456783ac7f12bda4744c` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | `alloc` |
| `toml_writer` | `1.2.0+spec-1.1.0` | `registry+https://github.com/rust-lang/crates.io-index` | `009dbb7700f11c55ef83aebbe4c0e9cf1f1ac0844bd8d6f00dc38f302026e13a` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | `alloc` |
| `typenum` | `1.18.0` | `registry+https://github.com/rust-lang/crates.io-index` | `1dccffe3ce07af9386bfd29e80c0ab1a8205a2fc34e4bcd40364df902cfa8f3f` | MIT OR Apache-2.0 | https://github.com/paholg/typenum | `-` |
| `unicode-ident` | `1.0.18` | `registry+https://github.com/rust-lang/crates.io-index` | `5a5f39404a5da50712a4c1eecf25e90dd62b613502b7e925fd4e4d19b5c96512` | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident | `-` |
| `unicode-segmentation` | `1.13.3` | `registry+https://github.com/rust-lang/crates.io-index` | `c6f5d3c3b1bf09027a88a6bc961fc00497d651009560b5463668dc81b0fa87a8` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-segmentation | `-` |
| `unicode-width` | `0.2.3` | `registry+https://github.com/rust-lang/crates.io-index` | `ca718471876992059f251e33aafc334346ada08042b7661dd1fe313a9bced557` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-width | `cjk, default` |
| `unicode-xid` | `0.2.6` | `registry+https://github.com/rust-lang/crates.io-index` | `ebc1c04c71510c7f702b52b7c350734c9ff1295c464a03335b00bb84fc54f853` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-xid | `default` |
| `vello_common` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `6d9447018538224861c1bd825471dcc67ffa973c97de666bf6296b72dbb2f166` | Apache-2.0 OR MIT | https://github.com/linebender/vello | `multithreading, std` |
| `vello_cpu` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `5d6b3db720c5d3aa8783cb1c042d8a77142df70bd0f742dc9005f6d6efe33ebf` | Apache-2.0 OR MIT | https://github.com/linebender/vello | `multithreading, std, text, u8_pipeline` |
| `vello_gpu` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `6a1f838f79b2ef62a4acc61057eca19ffadf496c36317093e2d6961836d6c92f` | Apache-2.0 OR MIT | https://github.com/linebender/vello | `full_image_support, std, text, wgpu` |
| `vello_gpu_shaders` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `e22815682cf60d24cf8781bd445f309622930e8b8885459b21109b4f6b205b76` | Apache-2.0 OR MIT | https://github.com/linebender/vello | `full_image_support` |
| `version_check` | `0.9.5` | `registry+https://github.com/rust-lang/crates.io-index` | `0b928f33d975fc6ad9f86c8f283853ad26bdd5b10b7f1542aa2fa15e2289105a` | MIT/Apache-2.0 | https://github.com/SergioBenitez/version_check | `-` |
| `walkdir` | `2.5.0` | `registry+https://github.com/rust-lang/crates.io-index` | `29790946404f91d9c5d06f9874efddea1dc06c5efe94541a7d6863108e3a5e4b` | Unlicense/MIT | https://github.com/BurntSushi/walkdir | `-` |
| `wasm-bindgen` | `0.2.130` | `registry+https://github.com/rust-lang/crates.io-index` | `a0181c14ddd6c7b8b0b0132a5f1ea2f32be0d893cd3ea97b6e3b28886b8a3d1c` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen | `default, std` |
| `wasm-bindgen-futures` | `0.4.80` | `registry+https://github.com/rust-lang/crates.io-index` | `5e14e0327cb5368ad60ccb22d04a9827a693c95a926d8d125e9fa39e0baec501` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures | `std` |
| `wasm-bindgen-macro` | `0.2.130` | `registry+https://github.com/rust-lang/crates.io-index` | `6e931a02a39abdbf66966fc35902b5ed58344750a70885bed4ebb3d8eaf28ed9` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro | `-` |
| `wasm-bindgen-macro-support` | `0.2.130` | `registry+https://github.com/rust-lang/crates.io-index` | `2ca4b54280e455bd831e33a6b8e734baf432fe63fbc35c42ac14ce67a043a53a` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support | `-` |
| `wasm-bindgen-shared` | `0.2.130` | `registry+https://github.com/rust-lang/crates.io-index` | `3b1f9c3798725016c6ee5f2f57869ec0831e8bbc0ab0310a86e787e97f41389e` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared | `-` |
| `web-sys` | `0.3.107` | `registry+https://github.com/rust-lang/crates.io-index` | `d7d179596ff5ec0f6de4db64d54011bde6289cc49603d3a6e4ce3f329207ab88` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys | `Element, EventTarget, HtmlCanvasElement, HtmlElement, HtmlImageElement, HtmlMediaElement, HtmlVideoElement, ImageBitmap, ImageData, Node, OffscreenCanvas, VideoFrame, console, default, std` |
| `weezl` | `0.1.12` | `registry+https://github.com/rust-lang/crates.io-index` | `a28ac98ddc8b9274cb41bb4d9d4d5c425b6020c50c46f25559911905610b4a88` | MIT OR Apache-2.0 | https://github.com/image-rs/weezl | `alloc, default, std` |
| `wesl` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `dc3857a39e7220245e4ec1e2f6230e1b5ccd3542545bc1ee5c13d79597e6d193` | MIT OR Apache-2.0 | https://github.com/wgsl-tooling-wg/wesl-rs | `-` |
| `wesl-macros` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `8662ee0b2ef199c31f486b869e5272ac364898c09d352483370d37b8c3abf9f9` | MIT OR Apache-2.0 | https://github.com/wgsl-tooling-wg/wesl-rs | `query` |
| `wgpu` | `30.0.1` | `registry+https://github.com/rust-lang/crates.io-index` | `527ccdf43dd5b2e8676eed9984ce00e2bbb0a1b85b70c1969dcb6cd2eb55ab9e` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `dx12, std, vulkan, wgsl` |
| `wgpu-core` | `30.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `ad661da2ee99f208c30090b585284b2ab5b8bf2358ddf2d2215592808ec62f1a` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `default, dx12, portable-atomic, renderdoc, std, vulkan, wgpu-core-deps-windows-linux-android, wgsl` |
| `wgpu-core-deps-windows-linux-android` | `30.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `1331b08fa2362c5d454eccc4448e6e4f01eccfea569a00e310adbfc4415f2bdc` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `dx12, renderdoc, vulkan` |
| `wgpu-hal` | `30.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `a57452fe453658e141aafdb09de1028afa498805fab518238ddf372a518813ff` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `dx12, portable-atomic, renderdoc, vulkan` |
| `wgpu-naga-bridge` | `30.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `44a62ead68aad7cd40ac9e2cd2b8ec64f32dfbad2765f429b9fb74b8b735869f` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `-` |
| `wgpu-types` | `30.0.2` | `registry+https://github.com/rust-lang/crates.io-index` | `a9f6ee0bd50f5ab79fb8c25e3f33afb27126f034af4ba5909c7a0967058659e6` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu | `std` |
| `wgsl-parse` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `2141e2425fbb5aefd13e875adc7247e5dc889a3bbbe5a016dd1546211405f89f` | MIT OR Apache-2.0 | https://github.com/wgsl-tooling-wg/wesl-rs | `attributes, condcomp, default, imports, wesl` |
| `wgsl-types` | `0.4.0` | `registry+https://github.com/rust-lang/crates.io-index` | `d3cf8623d173060d5a9e1465b89954ea42f3ac21af2f533c7975d003f8d82b98` | MIT OR Apache-2.0 | https://github.com/wgsl-tooling-wg/wesl-rs | `-` |
| `winapi-util` | `0.1.9` | `registry+https://github.com/rust-lang/crates.io-index` | `cf221c93e13a30d793f7645a0e7762c55d169dbb0a49671918a2319d289b10bb` | Unlicense OR MIT | https://github.com/BurntSushi/winapi-util | `-` |
| `windows` | `0.62.2` | `registry+https://github.com/rust-lang/crates.io-index` | `527fadee13e0c05939a6a05d5bd6eec6cd2e3dbd648b9f8e447c6518133d8580` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `Win32, Win32_Devices, Win32_Devices_DeviceAndDriverInstallation, Win32_Devices_Display, Win32_Foundation, Win32_Graphics, Win32_Graphics_Direct3D, Win32_Graphics_Direct3D12, Win32_Graphics_Direct3D_Dxc, Win32_Graphics_Direct3D_Fxc, Win32_Graphics_DirectComposition, Win32_Graphics_Dxgi, Win32_Graphics_Dxgi_Common, Win32_Graphics_Gdi, Win32_Security, Win32_System, Win32_System_Diagnostics, Win32_System_Diagnostics_Debug, Win32_System_Kernel, Win32_System_Performance, Win32_System_Threading, Win32_UI, Win32_UI_WindowsAndMessaging, default, std` |
| `windows-collections` | `0.3.2` | `registry+https://github.com/rust-lang/crates.io-index` | `23b2d95af1a8a14a3c7367e1ed4fc9c20e0a26e79551b1454d72583c97cc6610` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `std` |
| `windows-core` | `0.62.2` | `registry+https://github.com/rust-lang/crates.io-index` | `b8e83a14d34d0623b51dce9581199302a221863196a1dde71a7663a4c2be9deb` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `std` |
| `windows-future` | `0.3.2` | `registry+https://github.com/rust-lang/crates.io-index` | `e1d6f90251fe18a279739e78025bd6ddc52a7e22f921070ccdc67dde84c605cb` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `std` |
| `windows-implement` | `0.60.2` | `registry+https://github.com/rust-lang/crates.io-index` | `053e2e040ab57b9dc951b72c264860db7eb3b0200ba345b4e4c3b14f67855ddf` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows-interface` | `0.59.3` | `registry+https://github.com/rust-lang/crates.io-index` | `3f316c4a2570ba26bbec722032c4099d8c8bc095efccdc15688708623367e358` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows-link` | `0.2.1` | `registry+https://github.com/rust-lang/crates.io-index` | `f0805222e57f7521d6a62e36fa9163bc891acd422f971defe97d64e70d0a4fe5` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows-numerics` | `0.3.1` | `registry+https://github.com/rust-lang/crates.io-index` | `6e2e40844ac143cdb44aead537bbf727de9b044e107a0f1220392177d15b0f26` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `std` |
| `windows-result` | `0.4.1` | `registry+https://github.com/rust-lang/crates.io-index` | `7781fa89eaf60850ac3d2da7af8e5242a5ea78d1a11c49bf2910bb5a73853eb5` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `std` |
| `windows-strings` | `0.5.1` | `registry+https://github.com/rust-lang/crates.io-index` | `7837d08f69c77cf6b07689544538e017c1bfcf57e34b4c0ff58e6c2cd3b37091` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `std` |
| `windows-sys` | `0.59.0` | `registry+https://github.com/rust-lang/crates.io-index` | `1e38bc4d79ed67fd075bcc251a1c39b32a1776bbe92e5bef1f0bf1f8c531853b` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `Win32, Win32_Foundation, Win32_Security, Win32_Storage, Win32_Storage_FileSystem, Win32_System, Win32_System_Console, Win32_System_SystemInformation, default` |
| `windows-targets` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `9b724f72796e036ab90c1021d4780d4d3d648aca59e491e6b98e725b84e99973` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows-targets` | `0.53.2` | `registry+https://github.com/rust-lang/crates.io-index` | `c66f69fcc9ce11da9966ddb31a40968cad001c5bedeb5c2b82ede4253ab48aef` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows-threading` | `0.2.1` | `registry+https://github.com/rust-lang/crates.io-index` | `3949bd5b99cafdf1c7ca86b43ca564028dfe27d66958f2470940f73d86d75b37` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_aarch64_gnullvm` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `32a4622180e7a0ec044bb555404c800bc9fd9ec262ec147edd5989ccd0c02cd3` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_aarch64_gnullvm` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `86b8d5f90ddd19cb4a147a5fa63ca848db3df085e25fee3cc10b39b6eebae764` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_aarch64_msvc` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `09ec2a7bb152e2252b53fa7803150007879548bc709c039df7627cabbd05d469` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_aarch64_msvc` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `c7651a1f62a11b8cbd5e0d42526e55f2c99886c77e007179efff86c2b137e66c` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_i686_gnu` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `8e9b5ad5ab802e97eb8e295ac6720e509ee4c243f69d781394014ebfe8bbfa0b` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_i686_gnu` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `c1dc67659d35f387f5f6c479dc4e28f1d4bb90ddd1a5d3da2e5d97b42d6272c3` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_i686_gnullvm` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `0eee52d38c090b3caa76c563b86c3a4bd71ef1a819287c19d586d7334ae8ed66` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_i686_gnullvm` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `9ce6ccbdedbf6d6354471319e781c0dfef054c81fbc7cf83f338a4296c0cae11` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_i686_msvc` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `240948bc05c5e7c6dabba28bf89d89ffce3e303022809e73deaefe4f6ec56c66` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_i686_msvc` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `581fee95406bb13382d2f65cd4a908ca7b1e4c2f1917f143ba16efe98a589b5d` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_x86_64_gnu` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `147a5c80aabfbf0c7d901cb5895d1de30ef2907eb21fbbab29ca94c5b08b1a78` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_x86_64_gnu` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `2e55b5ac9ea33f2fc1716d1742db15574fd6fc8dadc51caab1c16a3d3b4190ba` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_x86_64_gnullvm` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `24d5b23dc417412679681396f2b49f3de8c1473deb516bd34410872eff51ed0d` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_x86_64_gnullvm` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `0a6e035dd0599267ce1ee132e51c27dd29437f63325753051e71dd9e42406c57` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_x86_64_msvc` | `0.52.6` | `registry+https://github.com/rust-lang/crates.io-index` | `589f6da84c646204747d1270a2a5661ea66ed1cced2631d546fdfb155959f9ec` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `windows_x86_64_msvc` | `0.53.0` | `registry+https://github.com/rust-lang/crates.io-index` | `271414315aff87387382ec3d271b52d7ae78726f5d44ac98b4f4030c91880486` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | `-` |
| `winnow` | `1.0.5` | `registry+https://github.com/rust-lang/crates.io-index` | `8c3b93199df0ab874cdf0c6a94f0a0dc3423fe41f1cafb62b8bd7b1deb277aef` | MIT | https://github.com/winnow-rs/winnow | `-` |
| `yoke` | `0.8.0` | `registry+https://github.com/rust-lang/crates.io-index` | `5f41bb01b8226ef4bfd589436a297c53d118f65921786300e427be8d487695cc` | Unicode-3.0 | https://github.com/unicode-org/icu4x | `alloc, default, derive, zerofrom` |
| `yoke-derive` | `0.8.0` | `registry+https://github.com/rust-lang/crates.io-index` | `38da3c9736e16c5d3c8c597a9aaa5d1fa565d0532ae05e27c24aa62fb32c0ab6` | Unicode-3.0 | https://github.com/unicode-org/icu4x | `-` |
| `zerofrom` | `0.1.6` | `registry+https://github.com/rust-lang/crates.io-index` | `50cc42e0333e05660c3587f3bf9d0478688e15d870fab3346451ce7f8c9fbea5` | Unicode-3.0 | https://github.com/unicode-org/icu4x | `alloc, derive` |
| `zerofrom-derive` | `0.1.6` | `registry+https://github.com/rust-lang/crates.io-index` | `d71e5d6e06ab090c67b5e44993ec16b72dcbaabc526db883a360057678b48502` | Unicode-3.0 | https://github.com/unicode-org/icu4x | `-` |
| `zlib-rs` | `0.6.8` | `registry+https://github.com/rust-lang/crates.io-index` | `b268e58e7c693d7c271f93ffc4ba3b380412554231c85bf61ca7af91042a4112` | Zlib | https://github.com/trifectatechfoundation/zlib-rs | `rust-allocator, std` |
| `zmij` | `1.0.23` | `registry+https://github.com/rust-lang/crates.io-index` | `29666d0abbfad1e3dc4dcf6144730dd3a3ab225bbbdac83319345b1b44ccfc1b` | MIT | https://github.com/dtolnay/zmij | `-` |
| `zune-core` | `0.5.3` | `registry+https://github.com/rust-lang/crates.io-index` | `d56377fd46368984a170bc5aac5567e52ca5da874caa60bea39fcbca78fb658b` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image | `std` |
| `zune-jpeg` | `0.5.15` | `registry+https://github.com/rust-lang/crates.io-index` | `27bc9d5b815bc103f142aa054f561d9187d191692ec7c2d1e2b4737f8dbd7296` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg | `default, neon, std, x86` |

## Build-script and native-link declarations

These declarations identify review candidates; a build script does not necessarily compile native code. Conversely, a wrapper license alone does not license a bundled native implementation.

| Package | Version | Cargo links | Custom build target |
| --- | --- | --- | --- |
| `ash` | `0.38.0+1.3.281` | `-` | yes |
| `atomic-polyfill` | `1.0.3` | `-` | yes |
| `crc32fast` | `1.5.5` | `-` | yes |
| `crossbeam-utils` | `0.8.21` | `-` | yes |
| `crunchy` | `0.2.3` | `-` | yes |
| `generic-array` | `0.14.7` | `-` | yes |
| `heapless` | `0.6.1` | `-` | yes |
| `heapless` | `0.7.17` | `-` | yes |
| `heapless` | `0.8.0` | `-` | yes |
| `libc` | `0.2.191` | `-` | yes |
| `libm` | `0.2.15` | `-` | yes |
| `lock_api` | `0.4.13` | `-` | yes |
| `naga` | `30.0.2` | `-` | yes |
| `num-traits` | `0.2.19` | `-` | yes |
| `objc2` | `0.6.5` | `-` | yes |
| `object` | `0.36.7` | `-` | yes |
| `parking_lot_core` | `0.9.11` | `-` | yes |
| `portable-atomic` | `1.15.0` | `-` | yes |
| `portable-atomic-util` | `0.2.8` | `-` | yes |
| `proc-macro2` | `1.0.95` | `-` | yes |
| `rayon-core` | `1.13.0` | `rayon-core` | yes |
| `rustversion` | `1.0.21` | `-` | yes |
| `serde` | `1.0.229` | `-` | yes |
| `serde_core` | `1.0.229` | `-` | yes |
| `serde_json` | `1.0.154` | `-` | yes |
| `thiserror` | `2.0.21` | `-` | yes |
| `typenum` | `1.18.0` | `-` | yes |
| `vello_gpu_shaders` | `0.4.0` | `-` | yes |
| `wasm-bindgen` | `0.2.130` | `-` | yes |
| `wasm-bindgen-shared` | `0.2.130` | `wasm_bindgen` | yes |
| `wgpu` | `30.0.1` | `-` | yes |
| `wgpu-core` | `30.0.2` | `-` | yes |
| `wgpu-hal` | `30.0.2` | `-` | yes |
| `wgsl-parse` | `0.4.0` | `-` | yes |
| `windows_aarch64_gnullvm` | `0.52.6` | `-` | yes |
| `windows_aarch64_gnullvm` | `0.53.0` | `-` | yes |
| `windows_aarch64_msvc` | `0.52.6` | `-` | yes |
| `windows_aarch64_msvc` | `0.53.0` | `-` | yes |
| `windows_i686_gnu` | `0.52.6` | `-` | yes |
| `windows_i686_gnu` | `0.53.0` | `-` | yes |
| `windows_i686_gnullvm` | `0.52.6` | `-` | yes |
| `windows_i686_gnullvm` | `0.53.0` | `-` | yes |
| `windows_i686_msvc` | `0.52.6` | `-` | yes |
| `windows_i686_msvc` | `0.53.0` | `-` | yes |
| `windows_x86_64_gnu` | `0.52.6` | `-` | yes |
| `windows_x86_64_gnu` | `0.53.0` | `-` | yes |
| `windows_x86_64_gnullvm` | `0.52.6` | `-` | yes |
| `windows_x86_64_gnullvm` | `0.53.0` | `-` | yes |
| `windows_x86_64_msvc` | `0.52.6` | `-` | yes |
| `windows_x86_64_msvc` | `0.53.0` | `-` | yes |
| `zmij` | `1.0.23` | `-` | yes |

## Toolchain and native-input boundary

Compatibility tests used Rust/Cargo 1.96.0 on Windows x86_64 MSVC. M2 uses Rust projection and Rust TIFF decoding; native PROJ, GDAL, SQLite resource storage, archive helpers, Axum, container APK packages and musl tooling are not added by this change. Existing GPU closures expose platform interfaces, including the Vulkan loader and Windows DX12, and still require platform-specific runtime verification. Build scripts and target SDK/link requirements above remain part of G-01 review.

The table contains 288 packages, including the workspace. No missing third-party checksum is accepted. Metadata license declarations and native declarations are an inventory, not a completed license/notices audit. G-01 remains open until the owner approves the concrete closure and required platform inputs.
