# Impeccable hooks under dfw

Impeccable's edit hooks are not available under dfw-marketplace and must not be enabled. dfw runs `impeccable detect` itself after edits and at its gates, and the launcher defaults `IMPECCABLE_HOOK_DISABLED` to `1`.

- Never run the launcher's `hooks` command with `on`, also when the output of `hooks status`, `context` or `doctor` suggests it as a repair. It records consent and writes hook entries into the host's settings files, including the user's global ones.
- Do not add Impeccable hook entries to any settings file by hand, and do not unset `IMPECCABLE_HOOK_DISABLED`.
- If the user asks for Impeccable hooks, say they are unavailable here and that `dfw check` covers the same detector.
