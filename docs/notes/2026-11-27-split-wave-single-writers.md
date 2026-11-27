# Single writers for split-wave LDS

When a Wave64 shader is lowered to Wave32 halves, one physical thread initializes the shared
ticket counter and one lane per half writes each shared exchange value. Every participating
thread still reaches the group barriers. These changes apply automatically to the split-wave
lowering. This change closes no TODO row.

## Whole-game shader validation

Compared merge base `18651dc9820b` with the complete code change `01f534f5096a`. Both builds
ran `leonida translate --list --validate --jobs 4` over the same Series X game directory and
staged executable. The directory and executable runs had disjoint reported program identities.
The CLI scans files up to 2 GiB for shader containers and skips duplicate code. All four scans
completed successfully.

Both builds used Linux with Proton 11.0 on an Intel Core i9-14900KS with 64 GB RAM. The runs
used the same base graphics profile, Wave32 host and maximum wave settings, and the Microsoft
DXIL validator from the 2026-09-29 DXC package. The validation ran on the CPU without pipeline
creation or GPU execution.

| Measure | Merge base | PR code |
|---|---:|---:|
| Containers found, including duplicates | 26,599 | 26,599 |
| Duplicate containers skipped | 614 | 614 |
| Native programs translated to IR | 26,148 | 26,148 |
| DXIL programs emitted | 25,754 | 25,754 |
| Vertex and pixel programs validated | 14,259 | 14,259 |
| Compute programs validated | 2,730 | 2,730 |
| Mesh programs validated | 8,694 | 8,694 |
| **Total programs validated** | **25,683** | **25,683** |
| Emitted programs rejected by the validator | 71 | 71 |

Every emitted program received a validator verdict. The same 25,683 programs passed on both
revisions, matched by full code hash and stage. The other reported outcomes were 71 mesh
validator rejections, four compute emission refusals, and 195 geometry resource-tracking
refusals on each revision. The CLI counted another 195 native programs without printing a
per-program outcome or emitting DXIL for them.

Comparing DXIL bytes for the same input containers found 249 changed programs, all compute.
All 249 passed validation before and after the change.

## GPU fixture and playtest

On an NVIDIA RTX 5090 with driver 580.173.02 and Proton 11.0, the authored exchange fixture
returned the expected OR result for five inputs through each of two branch forms on both
builds. Both captured fixtures passed SPIR-V validation. The fixed shader had one elected
writer per half.

Unattended Series X runs reached the opening courtyard. Bright and dark rectangles remained
visible with the full change active. Frame timing was unmeasured in this correctness test.
