---

### ⚠️ Antivirus note (false positive)

Windows Defender may quarantine `gamegene.exe` or flag it as **`Trojan:Win32/Wacatac.H!ml`**. **This is a false positive.**

GameGene reads and writes another process's memory — that's the whole point of a memory scanner — and to a heuristic/ML scanner that behaviour looks identical to a malicious injector, so it gets flagged. The `!ml` suffix means it's a machine-learning guess, not a real signature match. Unlike an actual Wacatac trojan, GameGene **makes no network connection, sets up no persistence, collects no data, and drops no other files** — its entire behaviour is local memory access against the process *you* attach to. It's also open source; you can read every syscall and build it yourself.

**Verify this download** by comparing its SHA-256 against the `.sha256` published beside each archive above:

```powershell
Get-FileHash -Algorithm SHA256 .\gamegene-<version>-windows-x86_64.zip   # Windows
```
```sh
sha256sum gamegene-<version>-linux-x86_64.tar.gz                          # Linux / macOS
```

Then paste that hash into `https://www.virustotal.com/gui/file/<sha256>` to see the community scan (typically only a couple of heuristic engines flag it — the fingerprint of a false positive).

Full explanation, and how to report the false positive to Microsoft: [README → Antivirus false positives](https://github.com/ragnarokmin0714/gameGene#antivirus-false-positives).
