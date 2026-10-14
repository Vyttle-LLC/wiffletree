## Context

One small fix. This records its cause, which took a reproduction to establish, and why the fix sits in the child.

## Cause

On macOS the standard library cannot make a pipe that is close-on-exec from the start, and it starts children with every descriptor that is inheritable at that instant. References are to Rust 1.97.0, the toolchain in use.

- `library/std/src/sys/pipe/unix.rs`: `pipe2(O_CLOEXEC)` is used on Linux, the BSDs and a few other systems. Every other system, macOS included, calls `pipe()` and then `set_cloexec()` on each end, which is one `ioctl(FIOCLEX)` each (`library/std/src/sys/fd/unix.rs`). Between those calls the two descriptors are inheritable.
- `library/std/src/sys/process/unix/unix.rs`: on Apple systems `Command::spawn` calls `posix_spawnp` and does not set `POSIX_SPAWN_CLOEXEC_DEFAULT`. `Stdio::piped()` makes its pipes inside `spawn` (`process/unix/common.rs`, `to_child_stdio`).

So when two threads spawn at once, one thread's child can be created while the other thread's pipes are made but not yet marked. Each provider turn is spawned on its own thread, and Git queries run on others.

Reproduction, on `main` at c13ff45 with only the new test added: `providers_started_together_hold_only_their_own_descriptors` starts eight stand-in providers together through `provider::configure`, 25 times, beside a Git query through `bounded_output`. Each provider lists its own descriptors.

| Build | Runs that failed | Providers holding a descriptor they were not given |
| --- | --- | --- |
| `main` | 30 of 30, the first run included | 459 of 6,000 |
| `main`, spawns behind one lock and no Git query | 0 of 20 | 0 of 4,000 |
| with this fix | 0 of 250 | 0 of 50,000 |

The extra descriptors came in ones and twos with numbers such as 26 and 27, or 48 and 49: the two ends of another spawn's pipe. The middle row shows they are another in-flight spawn's pipes and nothing else. It was measured with an earlier form of the test that ran the same rounds.

## Decisions

### 1. The child marks its inherited descriptors, the host does not serialize spawns
`own_descriptors_only` adds a hook that runs in the child between fork and exec and sets close-on-exec on every descriptor above stderr. The child reads its own descriptor list with `proc_pidinfo(PROC_PIDLISTFDS)` into a fixed buffer of 1,024 entries on its stack.

The hook runs in a copy of a process that had other threads, any of which may have held a lock, so it makes system calls only (`getpid`, `proc_pidinfo`, `fcntl`, `getdtablesize`): it does not allocate, lock, log or have a path that panics.

If the list call fails, or the list fills the buffer and so may be cut short, the hook marks every number from 3 up to the descriptor limit instead. No descriptor stays inheritable in either case; the cost is time. The limit was 92,160 in a plain process, where that loop took about 8 ms, and 1,048,576 in the shell these tests ran from. The host would need 1,024 or more descriptors open at once to take that path.

*Alternative:* one lock around every spawn. Rejected because it protects only the spawn sites that take it, and only against pipes made by another spawn. The hook makes each child safe whatever else in the process holds a descriptor and however it was made.

*Alternative:* `POSIX_SPAWN_CLOEXEC_DEFAULT`. It is the system's own answer, but the standard library does not expose spawn attributes, so it would mean replacing `Command` with direct `posix_spawn` calls at every site.

### 2. Every process the host starts, not only provider turns
The hook is applied at all six places the host starts a process: provider turns and compactions (`provider::configure`), `bounded_output` (Git, `gh`, CLI detection), the Codex app-server probe, the two Claude probes in `runtime.rs` and the Claude context probe. The provider turn alone would keep a provider from holding another's pipe. The others matter in the opposite direction: a `git push` may run its hooks for ten minutes, and a probe is a provider CLI, so either could hold a provider's prompt pipe open for that long. Each site is one line.

The desktop app's own spawns (`git`, `codesign`, `curl`, the login shell) are left alone. They run in another process, which makes no provider pipes.

## Risks / Trade-offs

- A hook before exec makes the standard library use fork and exec in place of `posix_spawn`, and fork copies the memory map of the process it runs in. The host runs inside the desktop app, which had about 7,000 memory regions and a 295 MB footprint when measured. A scratch program with the same hook was given that shape, 7,000 separate regions with 328 MB touched, and timed over 100 spawns each way on a release build:

  | | Provider-style `spawn()` | One `git --version`, start to exit |
  | --- | --- | --- |
  | `posix_spawn`, before | 0.14 ms | 4.2 ms |
  | fork and hook, after | 2.0 ms | 5.7 ms |
  | after, with 656 MB touched | 3.6 ms | 7.2 ms |

  These are medians. With no added memory the hook cost 0.6 ms. So a spawn costs about 2 ms more in a process of the app's size, and the cost grows with the app's memory. A provider start takes seconds. The periodic Git work is a few calls per watched repository each minute from the PR watcher and a fetch with a few queries per repository every five minutes from the overlap check, and a minute of sampling the running app's children every 75 ms, with agents working, saw no `git` at all. The time the parent spends on copy-on-write faults after each fork was not measured.
- Standard input, output and error are inherited as before. A process that wants to hand a provider anything else must now do so on purpose.
- Other systems take the fallback loop. Only macOS was built and tested.
