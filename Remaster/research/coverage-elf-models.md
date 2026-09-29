# Format coverage

All loose files; recursively expanded BIG/VIV and U8 containers. Identical container content expanded once.

A recognized header or a successful export is not a complete decode.

| Extension | Records | Formats | Statuses |
|---|---:|---|---|
| .abk | 21 | EA audio bank | recognized: 21 |
| .anm | 4 | ELF32 / EA relocatable | structural: 4 |
| .apt | 655 | EA APT UI program | recognized: 655 |
| .arc | 10 | Nintendo U8 archive | structural: 10 |
| .asf | 14 | EA audio stream | recognized: 14 |
| .ast | 5 | EA audio stream | recognized: 5 |
| .atd | 3 | unknown binary | unknown: 3 |
| .bh | 1 | EA BIG external directory | structural: 1 |
| .big | 656 | EA BIG archive | structural: 656 |
| .bin | 10 | unknown binary | unknown: 10 |
| .bnk | 10 | EA audio bank | recognized: 10 |
| .bnr | 1 | unknown binary | unknown: 1 |
| .brfnt | 8 | Nintendo font | structural: 8 |
| .brlan | 367 | Nintendo layout animation | structural: 367 |
| .brlyt | 44 | Nintendo layout | structural: 44 |
| .brsar | 1 | Nintendo sound archive | recognized: 1 |
| .bts | 1 | Text | decoded_text: 1 |
| .bwav | 5 | unknown binary | unknown: 5 |
| .con | 32 | unknown binary | unknown: 32 |
| .const | 655 | EA APT constants | recognized: 655 |
| .cpt | 10 | unknown binary | unknown: 10 |
| .csi | 2 | EA audio metadata | recognized: 2 |
| .csv | 50 | Delimited table | decoded_text: 50 |
| .dat | 8 | EA audio stream | recognized: 8 |
| .dol | 2 | Nintendo DOL executable | structural: 2 |
| .elf | 1 | ELF32 executable | structural: 1 |
| .evt | 1 | unknown binary | unknown: 1 |
| .gfn | 16 | EA font | recognized: 16 |
| .gsh | 805 | EA GSH texture archive | partial: 805 |
| .gsm | 1 | unknown binary | unknown: 1 |
| .hdr | 8 | unknown binary | unknown: 8 |
| .hkx | 74 | Havok packfile | structural: 74 |
| .idx | 2 | EA localization index | partial: 2 |
| .img | 1 | unknown binary | unknown: 1 |
| .ini | 2 | Text | decoded_text: 2 |
| .lef | 241 | LION effect tree | structural: 241 |
| .loc | 10 | EA localization | partial: 10 |
| .mkr | 49 | unknown binary | unknown: 49 |
| .o | 863 | ELF32 / EA relocatable | structural: 863 |
| .ske | 4 | ELF32 / EA relocatable | structural: 4 |
| .tpl | 464 | Nintendo TPL | partial: 464 |
| .txt | 5 | Text | decoded_text: 5 |
| .viv | 118 | EA BIG archive | structural: 118 |
| .vlt | 1 | EA VLT database | recognized: 1 |
| .vp6 | 8 | EA video stream | recognized: 8 |
| .znd | 9 | unknown binary | unknown: 9 |
| .zsd | 9 | unknown binary | unknown: 9 |

## Payload decoder checks

These are distinct file contents, separate from header/structure status above.

| Extension | Unique files | Payload results |
|---|---:|---|
| .anm | 4 | partial: 4 |
| .gsh | 736 | partial: 736 |
| .o | 836 | partial: 828, structural: 8 |
| .ske | 4 | partial: 4 |
| .tpl | 199 | partial: 199 |

Full paths, signatures, hashes, payload checks and parser errors are in the JSON catalog.

Traversal errors: 0.
