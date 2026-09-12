// A slow read must never overwrite a mutation that started after that read.
export class SnapshotGuard {
    private revision = 0;
    private mutations = 0;
    private latestRead = 0;
    private mutationTail: Promise<void> = Promise.resolve();

    async read<T>(operation: () => Promise<T>): Promise<T | null> {
        if (this.mutations > 0) return null;
        const revision = this.revision;
        const read = ++this.latestRead;
        const result = await operation();
        return this.mutations === 0 && this.revision === revision && read === this.latestRead
            ? result
            : null;
    }

    mutate<T>(operation: () => Promise<T>): Promise<T> {
        this.revision += 1;
        this.mutations += 1;
        const result = this.mutationTail.then(operation).finally(() => {
            this.mutations -= 1;
            this.revision += 1;
        });
        this.mutationTail = result.then(
            () => undefined,
            () => undefined,
        );
        return result;
    }
}
