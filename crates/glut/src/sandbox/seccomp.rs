//! The Linux sandbox's seccomp filter (T59), Glut's own: one classic BPF program the kernel runs on every system call
//! of a command. Built on every system — its numbers handed in — so a test runs it through a small interpreter on the
//! Mac too; only [`super::linux`] loads it.
//!
//! What it says, in order:
//! - a call of another architecture than the one it was built for (i386 on x86_64, AArch32 on arm64) kills the
//!   process: its numbers mean other calls;
//! - on x86_64 an x32 call (bit 30 of the number) is "not here": it would name the same calls by numbers the rest
//!   never checks;
//! - the calls that make a sandbox inside the sandbox, reach into another process, or slip past the filter
//!   (`io_uring`) are refused;
//! - `clone3` is "not here" (its flags lie in memory a filter cannot read; the C library falls back to `clone`), and
//!   `clone` with a flag for a new namespace is refused;
//! - a socket of any family but IPv4, IPv6 and netlink is refused — a Unix one reaches the owner's agents, a vsock the
//!   host of a VM;
//! - everything else is allowed.

/// The kernel's answers ([`seccomp(2)`]).
const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const RET_ALLOW: u32 = 0x7fff_0000;

/// The classic BPF instructions the program uses.
const LD_W_ABS: u16 = 0x20;
const JEQ_K: u16 = 0x15;
const JSET_K: u16 = 0x45;
const RET_K: u16 = 0x06;

/// Where `seccomp_data` keeps the call's number, its architecture and the low half of its first argument (on a
/// little-endian machine, the only kind the filter is built for).
const NUMBER_AT: u32 = 0;
const ARCH_AT: u32 = 4;
const FIRST_ARGUMENT_AT: u32 = 16;

/// The x32 ABI's mark in a call's number.
const X32_BIT: u32 = 0x4000_0000;

/// One instruction, laid out as the kernel's `struct sock_filter`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    /// What the instruction does (load, jump, return).
    pub code: u16,
    /// How many instructions a jump skips when its test holds.
    pub jt: u8,
    /// How many it skips when the test fails.
    pub jf: u8,
    /// The value loaded from, compared with or returned.
    pub k: u32,
}

/// The numbers a filter is built from: those of the machine it runs on.
#[derive(Debug, Clone)]
pub struct Calls {
    /// `AUDIT_ARCH_*` of the machine.
    pub arch: u32,
    /// Whether the kernel may know the x32 ABI (x86_64).
    pub x32: bool,
    /// Refused outright (`EPERM`).
    pub refused: Vec<u32>,
    /// Answered "not here" (`ENOSYS`).
    pub absent: Vec<u32>,
    /// `socket`'s number: its family is judged.
    pub socket: u32,
    /// `clone`'s number: its flags are judged.
    pub clone: u32,
    /// The socket families allowed.
    pub families: Vec<u32>,
    /// The `clone` flags that make a new namespace.
    pub namespace_flags: u32,
    /// `EPERM`, the answer to what is refused.
    pub eperm: u32,
    /// `ENOSYS`, the answer to what is "not here".
    pub enosys: u32,
}

/// The program for `calls`.
pub fn program(calls: &Calls) -> Vec<Instruction> {
    let step = |code, jt, jf, k| Instruction { code, jt, jf, k };
    let ret = |k| step(RET_K, 0, 0, k);
    let (refuse, absent) = (RET_ERRNO | calls.eperm, RET_ERRNO | calls.enosys);
    let mut out = vec![
        step(LD_W_ABS, 0, 0, ARCH_AT),
        step(JEQ_K, 1, 0, calls.arch),
        ret(RET_KILL_PROCESS),
        step(LD_W_ABS, 0, 0, NUMBER_AT),
    ];
    if calls.x32 {
        out.extend([step(JSET_K, 0, 1, X32_BIT), ret(absent)]);
    }
    // Each check: the next instruction answers on a match, the one after it goes on.
    for number in &calls.refused {
        out.extend([step(JEQ_K, 0, 1, *number), ret(refuse)]);
    }
    for number in &calls.absent {
        out.extend([step(JEQ_K, 0, 1, *number), ret(absent)]);
    }
    // `socket`: its family, the first argument. A listed family jumps to the allow at the block's end.
    let families = u8::try_from(calls.families.len()).unwrap_or(u8::MAX);
    out.push(step(JEQ_K, 0, families + 3, calls.socket));
    out.push(step(LD_W_ABS, 0, 0, FIRST_ARGUMENT_AT));
    for (at, family) in calls.families.iter().enumerate() {
        let to_allow = families - u8::try_from(at).unwrap_or(u8::MAX);
        out.push(step(JEQ_K, to_allow, 0, *family));
    }
    out.extend([ret(refuse), ret(RET_ALLOW)]);
    // `clone`: its flags, the first argument on every machine the filter is built for.
    out.extend([
        step(JEQ_K, 0, 4, calls.clone),
        step(LD_W_ABS, 0, 0, FIRST_ARGUMENT_AT),
        step(JSET_K, 0, 1, calls.namespace_flags),
        ret(refuse),
        ret(RET_ALLOW),
    ]);
    out.push(ret(RET_ALLOW));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `program` on one call as the kernel would: what it answers.
    fn run(program: &[Instruction], arch: u32, number: u32, first: u32) -> u32 {
        let mut accumulator = 0u32;
        let mut at = 0usize;
        loop {
            let step = program[at];
            at += 1;
            match step.code {
                LD_W_ABS => {
                    accumulator = match step.k {
                        NUMBER_AT => number,
                        ARCH_AT => arch,
                        FIRST_ARGUMENT_AT => first,
                        other => panic!("load at {other}"),
                    }
                }
                JEQ_K | JSET_K => {
                    let hit = if step.code == JEQ_K {
                        accumulator == step.k
                    } else {
                        accumulator & step.k != 0
                    };
                    at += usize::from(if hit { step.jt } else { step.jf });
                }
                RET_K => return step.k,
                other => panic!("instruction {other:#x}"),
            }
        }
    }

    fn calls(x32: bool) -> Calls {
        Calls {
            arch: 0xC000_003E,
            x32,
            refused: vec![272, 308, 425],
            absent: vec![435],
            socket: 41,
            clone: 56,
            families: vec![2, 10, 16],
            namespace_flags: 0x1000_0000 | 0x0002_0000,
            eperm: 1,
            enosys: 38,
        }
    }

    #[test]
    fn every_call_gets_the_answer_the_rules_give() {
        let built = calls(true);
        let program = program(&built);
        let arch = built.arch;
        let refused = RET_ERRNO | 1;
        let absent = RET_ERRNO | 38;
        assert_eq!(run(&program, arch, 0, 0), RET_ALLOW, "a plain read");
        assert_eq!(
            run(&program, 0x4000_0003, 0, 0),
            RET_KILL_PROCESS,
            "another architecture"
        );
        assert_eq!(run(&program, arch, 272, 0), refused, "unshare");
        assert_eq!(run(&program, arch, 425, 0), refused, "io_uring_setup");
        assert_eq!(run(&program, arch, 435, 0), absent, "clone3");
        assert_eq!(
            run(&program, arch, 41 | X32_BIT, 1),
            absent,
            "socket by its x32 number"
        );
        assert_eq!(run(&program, arch, X32_BIT, 0), absent, "any x32 call");
        for (family, answer) in [
            (1, refused),
            (2, RET_ALLOW),
            (10, RET_ALLOW),
            (16, RET_ALLOW),
            (40, refused),
        ] {
            assert_eq!(
                run(&program, arch, 41, family),
                answer,
                "socket of family {family}"
            );
        }
        assert_eq!(
            run(&program, arch, 56, 0x0001_0000),
            RET_ALLOW,
            "clone of a thread"
        );
        assert_eq!(
            run(&program, arch, 56, 0x1000_0000 | 0x0001_0000),
            refused,
            "clone into a new user namespace"
        );
        assert_eq!(
            run(&program, arch, 56, 0x0002_0000),
            refused,
            "clone into a new mount namespace"
        );
        assert_eq!(
            run(&program, arch, 57, 0x1000_0000),
            RET_ALLOW,
            "another call with those bits"
        );
    }

    #[test]
    fn without_x32_its_bit_is_no_rule_of_its_own() {
        let built = calls(false);
        let program = program(&built);
        assert_eq!(run(&program, built.arch, X32_BIT, 0), RET_ALLOW);
        assert_eq!(
            program.len(),
            calls(true).refused.len() * 2 + 4 + 2 + 2 + 3 + 2 + 5 + 1
        );
    }
}
