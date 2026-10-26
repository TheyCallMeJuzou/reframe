// Waiting with a limit, for things that may never finish (a process that hangs, a share that does not answer).

/** Whether `promise` settles within `ms` (fulfilled or rejected); it is not stopped either way. */
export function settle(promise: Promise<unknown>, ms: number): Promise<boolean> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), ms);
    const done = () => {
      clearTimeout(timer);
      resolve(true);
    };
    promise.then(done, done);
  });
}
