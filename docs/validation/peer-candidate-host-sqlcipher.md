# Guarded host SQLCipher qualification

Linux AMD64, source `4fef8d9f81e8c476e68ba93d25517db4fe5cf6ca`.
The candidate library test executable had SHA-256
`d7737aee3593a0c2f94beb010e97eada5086b7440305b25a9c12b1b3145c3ed1`.
The registration suite passed its 22 fresh subprocess cases in 98.54 s.
These include healthy forwarding, refusal at raw open or keying, refusal
at an explicit close and refusal before the next read-only scan. A real
default displacement triggered the same registered guard used by Command.

The five native preflight cases passed in 4.08 s and all 26 Journal cases
passed in 11.51 s. The guarded Journal test committed and rolled back
updates and checked that Deferred transactions could begin during a
competing write lock. The database ciphertext contained no test plaintext
after the final close. Strict Clippy passed for both native and engine
all targets in candidate (39.17 s) and stock (30.15 s) configurations.
Stock engine library tests reported 154 passed and one ignored in 29.45 s.

This covers Marmish's host Journal and read-only compatibility scanner.
The upstream MDK account owner and native lifecycle remain separate
qualification scopes.
