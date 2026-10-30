## 1. Host

- [x] 1.1 Recognise a limit failure at turn end: a quota reading during the turn first, the error text otherwise.
- [x] 1.2 Record `limit_retry` on the runtime and tell the parent once that nothing is needed.
- [x] 1.3 Release the held input in the scheduler's pass once the limit has reset, reusing the reset wake and the pass on every reading.
- [x] 1.4 End the wait on Retry, Skip and the next turn.

## 2. Desktop, text and tests

- [x] 2.1 The held-input banner says when the input is retried.
- [x] 2.2 Coordinator core v7 and the `resume_agent` description.
- [x] 2.3 Test the retry at a known reset and after a restart, an unknown reset, error text alone, Skip, other failures and a verifier in a running cycle.
