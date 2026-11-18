# Recipient clean quota turn review

The [charge scope design](peer-receiver-charge-scope.md) was approved before implementation. This code increment covers the local Linux AMD64 receiver file exclusion and measured original and snapshot charge. The [capped Pending bridge](../phases/peer-installed-capped-pending-bridge.md) owns native Welcome and durable Pending authority.

## Round 1

Claude Code Sonnet 5.5 in Auto mode reviewed `4a55c15eb05f4e35ced3bc70949b7944c787d4ab` in session `ab08eedc-86da-4221-b2df-3c198dab705e` and requested changes. The initial test verified measurement and capability construction. It did not enter the new snapshot dispatcher or close a keyed database under its quota. The reviewer accepted the two fixed prepare and read wrappers sharing the same lower measured dispatcher. The reviewer ran a compile-only check because the exact test executable was still linking at first inspection.

## Round 2

Claude Code Sonnet 5.5 in Auto mode reviewed `75dc953396e5ce79608b1f64ed90b8f36d32f328` relative to `25748b2` in session `b46cd0d7-1986-41a6-9e90-0a72df56704c` and **approved** with no blocking findings. The revised subprocess prepares a keyed DELETE-mode SQLCipher image and verifies a native query and close through the actual protected snapshot reservation. It exercises exact capability kind, stale post-measure extent, the competing rollback path, and the one-byte margin at the reservation boundary.

The reviewer verified the recorded executable checksum, ran the exact measured copy case in 0.54 s and managed rollback case in 2.83 s, and saw four passing deterministic inventory tests. Invoking the whole namespace with `--ignored` additionally invoked the private child without its required fixture environment. That child reported `NotPresent`. Both opt-in parent cases passed separately and their genuine child invocations succeeded. [Author validation](../validation/peer-receiver-clean-charge.md)

The cap-minus-one success is covered at reservation time. The actual protected open uses the smaller real copied image, and the exact cap is also rejected after file creation. Existing dispatcher admission compares the current device and inode to the measured witness before opening, and managed VFS methods retain their identity checks. A same-size replacement regression and a wrong-key clean-copy check remain optional additional coverage. The original turn and ordinary stock cursor continue to use the same cooperating profile lease and file mutex. The fixed copy checks do not confer native Welcome or Pending authority.

Ripwire highlighted repeated short typed wrappers, normalized-token matches across unrelated code, and near-term churn in the same inventory region. The reviewer accepted keeping the typed prepare and read methods over their shared dispatcher. The additional file-system and SQLCipher test remains opt-in. No installed route or CI defaults change.

## Integrated review

Claude Code Sonnet 5.5 in Auto mode reviewed `d26c570..b2a0ccc2f2157c93ce58bac523db66ba3499b521` in session `a31afd14-b030-4abe-91b3-befb76ebbb20` and **approved** without blocking findings. The reviewer verified byte parity for all five Rust files with `75dc953` and both initial evidence files with `462f339`. The reviewer confirmed the composed executable's checksum and independently passed the clean copy in 0.86 s, managed rollback in 3.15 s, and four deterministic inventory tests. [Composed author validation](../validation/peer-receiver-clean-charge.md#composed-linux-validation)
