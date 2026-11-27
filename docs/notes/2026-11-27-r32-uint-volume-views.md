# Exact integer values in owned volume views

The ordinary image path declares float resources even when the native view is an integer
texture. An original, asset-free program reproduced a lost high bit on an RTX 5090: a
literal `0x80000000` stored through the float path into `R32_UINT` became zero. A proper
integer store preserved it. Runtime values and the other tested special bit patterns did
not reproduce that loss on this driver; the test keeps those as independent controls.

`LEONIDA_UINT_IMAGES=1` adds a bounded, opt-in correction. It is off by default. A static
ordinary Texture3D load of R, or a full-mask store, gets mutually exclusive float and
integer paths. The host selects integer access only from the final explicit native
`R32_UINT` view in a private descriptor allocation owned by the recording. The allocation
and its heap survive repeated submissions until that recording is reset or dropped.
Descriptor rewrites, null/fallback creation, release and reuse invalidate its class.
The store retains the original EXEC guard and writes one raw integer component.

The admission rules require one mip, with default SRV component mapping for loads or a
complete UAV depth range for stores, and no 16-bit coordinates/data or dynamic descriptor
selection. Ordinary mirrored tables, typeless resources, inferred descriptions and other
view shapes retain the legacy path. Root class words share the existing allocator and cache
encoding; malformed manifests and a root signature exceeding 64 DWORDs are refused.
No program hashes or game-specific bindings select the correction.

The new source and cache encoding invalidate older translations even with the switch off;
the two authored disabled emissions remained byte-identical to the base. The explicit
64-DWORD check also applies with the switch off and rejects root signatures that already
exceed the native limit. No descriptor allocation is pinned by this feature when it is off.

## Measurements

Core i9-14900KS, 64 GB RAM, RTX 5090, NVIDIA 580.173.02, Proton 11.0. The actual Vulkan
device was checked separately from DXGI's reported adapter identity. The device test used
an isolated hidden display without sound or network access.

| Authored 4 by 2 by 13 volume | Legacy float path | Integer path |
|---|---:|---:|
| Literal high-bit stores, SRV-input variant | 104/104 wrong words | 0/104 wrong words |
| Literal high-bit stores, UAV-input variant | 104/104 wrong words | 0/104 wrong words |

The final program runs 32 cases: separate uploaded image loads and computed, runtime-buffer
or literal stores, each with SRV/UAV inputs and float/integer controls. All 3,328 checked
integer-path words matched. The float controls reproduced the two failures above; their
other checked words matched. Post-fence checks exercised real native view creation,
category mismatch, null replacement, failed creation, recreation and allocation release.
There was no GPU fault or new validation error. The test took 1.27 seconds; that is a test
duration, not a gameplay performance measurement.

The GPU fixture uses the real descriptor creators and numeric-class query, then supplies
the root constants directly. It does not exercise `State::bind`, asynchronous retirement
or repeated submission end to end. A separate recording-retention CPU regression passed
under Windows/Proton, alongside source review. Keeping eligible tables until reset can
increase descriptor pressure while enabled; that cost has not been measured in gameplay.

Native shader tests passed (220, with two existing ignores), as did eight tests of the
production cache codec. Windows workspace strict Clippy passed with default and all
features, and every Windows workspace test binary compiled. Two original disabled
programs exactly matched upstream bytecode; four enabled/disabled conversions passed
SPIR-V validation. Thirteen exact Windows/Proton CPU tests passed without skips: three
host contracts, eight cache tests and two original-program emission tests. The
Linux-native workspace and GPU test commands still fail on the
same Windows-only references as the exact upstream base; those failures were compared
and were not changed here.

## Gameplay boundary

This is a reproduced integer-storage correction, not an artifact-free gameplay result.
The longer unmodified-main run still showed corrupt lighting and later hit a GPU timeout.
The inspected lighting stores did not contain the directly failing literal pattern, and
some use mirrored tables outside this correction's scope. No gameplay frame-rate gain or
fix for those visible blocks is claimed. R-9 and the other rendering TODO rows remain open.

No existing TODO row is closed by this narrowly scoped fix. All programs and values in
the committed regression tests are original; game-derived captures and shader output
remain outside the repository.
