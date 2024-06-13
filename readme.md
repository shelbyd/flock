# flock

Flock is a VM and family of programming languages designed around running processes across arbitrarily large clusters of machines, while appearing to run on a single, arbitrarily powerful machine.

Flock is currently in active development. Don't expect these features to work or be stable if they happen to work.

## VM

The VM is composed of multiple machines and disks acting as a single machine with all the resources of the backing machines available.

Users create a VM with `flock new $VM_ID`.

Users add machines/resources to the VM with: 

```sh
flock join <vm_id>@<ip>:<port> \
    --provide cpu:3,disk:32GiB,ram:95% \
    --data ~/.flock_data
```

Users can then spawn a process with `flock run <file>.fl --vm <vm_id>@<ip>:<port>`.

### Execution Structure

VMs have 0 or more processes running at any time. All the processes see the same permanent storage. Each process has its own global memory space, shared across all threads in the process.

A process has one or more threads. Each thread has a local memory space structured the same as the global memory space, but writes are only reflected in the current thread and spawned threads (at the moment of spawning).

Each thread also has a stack of 64bit words for execution. The VM instructions are stack oriented and operate primarily on the top of the stack.

Threads are very light, so should be created judiciously.

Compute and data are dynamically distributed so that compute is done near the data it uses.

### Memory

Global and local memory share the same address space. Address MSB == 1 indicates process global address space, and MSB == 0 indicates thread-local.

Memory is allocated in multiples of 8 bytes, matching the word size of task's stacks. Individual bytes can be addressed, but `ALLOC N` will always round up N such that N % 8 == 0.

### Permanent Storage

The VM provides instructions to write to and read from permanent storage. Each VM has a single filesystem.

Data is moved between machines to guarantee redundancy requirements specified in a directory's metadata. Users can also restrict reading and/or writing of directories to specific users.

### Synchronization

The VM provides instructions to support synchronization across threads. This includes Compare-and-Swap and Memory Fences.

### At Most Once

# Open Questions

- ~~Does the VM or user space implement memory allocation?~~ Likely VM, since the VM can optimize sharing across machines.
- ~~Is permanent storage modeled as blocks or as a filesystem?~~
    - Probably filesystem, since user programs may expect block storage to have more guarantees than it actually does.
    - We may want permissions on certain storage regions.
    - We want different rules for redundancy of different storage regions.
- Do we need more synchronization primitives?
    - Perhaps Mutexes to deal with terminated machines?
