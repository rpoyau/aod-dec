// Generic supplied-table execution: no intrinsic probability or geometry law.
// One thread owns one path. All arithmetic and ticket comparisons are integer.
extern "C" __global__ void chiral_batch(
    const unsigned int* movement, const unsigned int* thresholds,
    const int* initial, const unsigned int* tickets, int* output,
    unsigned int paths, unsigned int steps) {
    unsigned int path = blockIdx.x * blockDim.x + threadIdx.x;
    if (path >= paths) return;
    int a = initial[path * 4], b = initial[path * 4 + 1];
    int ca = initial[path * 4 + 2], cb = initial[path * 4 + 3];
    for (unsigned int k = 0; k < steps; ++k) {
        unsigned int input = (path * steps + k) * 4;
        a = movement[a * 8 + tickets[input]];
        b = movement[b * 8 + tickets[input + 1]];
        int fa = tickets[input + 2] < thresholds[a * 8 + b];
        int fb = tickets[input + 3] < thresholds[b * 8 + a];
        ca = fa ? -ca : ca;
        cb = fb ? -cb : cb;
        unsigned int out = (path * steps + k) * 7;
        output[out] = a; output[out + 1] = b;
        output[out + 2] = ca; output[out + 3] = cb;
        output[out + 4] = fa; output[out + 5] = fb;
        output[out + 6] = a == b;
    }
}
