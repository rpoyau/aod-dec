// Exact incidence of a validated occurrence family on declared boundary views.
// No rational values, probabilities, witness admission or atomic reductions.
extern "C" __global__ void boundary_incidence(
    const int* source, const int* target, const int* event_cut,
    const int* available, const int* kind,
    const int* before, const int* after,
    const int* sweep_cell, const int* sweep_cut,
    int* event_output, int* sweep_output,
    unsigned int events, unsigned int sweeps, unsigned int cells) {
    unsigned int row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row < events) {
        unsigned int offset = (unsigned int)event_cut[row] * cells;
        int a = after[offset + source[row]], b = after[offset + target[row]];
        int orientation = a - b;
        int selected = available[row] && orientation != 0;
        unsigned int out = row * 6;
        event_output[out] = orientation;
        event_output[out + 1] = selected;
        event_output[out + 2] = a && b;
        event_output[out + 3] = !a && !b;
        event_output[out + 4] = !available[row];
        // 0 inactive, 1 signed crossing, 2 signed shedding, 3 unresolved burden.
        event_output[out + 5] = selected ? kind[row] + 1 : 0;
    }
    if (row < sweeps) {
        unsigned int position = (unsigned int)sweep_cut[row] * cells + sweep_cell[row];
        sweep_output[row] = after[position] - before[position];
    }
}
