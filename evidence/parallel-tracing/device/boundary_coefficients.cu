// dec-cuda-int64-bool-v1; generated from verified Dec IR

typedef long long DecInt;
typedef unsigned long long DecUInt;
static const DecInt DEC_MAX = 9223372036854775807LL;
static const DecInt DEC_MIN = (-9223372036854775807LL - 1LL);
struct DecContext { DecInt fuel; int error; int line; };
__device__ bool dec_gpu_tick(DecContext* c) {
    if (c->fuel <= 0) { c->error = 1; c->line = 0; return false; }
    --c->fuel; return true;
}
__device__ DecInt dec_gpu_domain(DecContext* c, int line) {
    c->error = 4; c->line = line; return 0;
}
__device__ DecInt dec_gpu_add(DecContext* c, DecInt a, DecInt b, int line) {
    if ((b > 0 && a > DEC_MAX-b) || (b < 0 && a < DEC_MIN-b))
        return dec_gpu_domain(c,line);
    return a+b;
}
__device__ DecInt dec_gpu_sub(DecContext* c, DecInt a, DecInt b, int line) {
    if ((b < 0 && a > DEC_MAX+b) || (b > 0 && a < DEC_MIN+b))
        return dec_gpu_domain(c,line);
    return a-b;
}
__device__ DecUInt dec_gpu_magnitude(DecInt a) {
    return a < 0 ? (DecUInt)(-(a+1))+1ULL : (DecUInt)a;
}
__device__ DecInt dec_gpu_mul(DecContext* c, DecInt a, DecInt b, int line) {
    const bool negative = (a < 0) != (b < 0);
    const DecUInt x = dec_gpu_magnitude(a), y = dec_gpu_magnitude(b);
    const DecUInt limit = negative ? (1ULL << 63) : (DecUInt)DEC_MAX;
    if (y && x > limit/y) return dec_gpu_domain(c,line);
    const DecUInt product = x*y;
    if (!negative) return (DecInt)product;
    if (product == (1ULL << 63)) return DEC_MIN;
    return -(DecInt)product;
}
__device__ DecInt dec_gpu_neg(DecContext* c, DecInt a, int line) {
    if (a == DEC_MIN) return dec_gpu_domain(c,line);
    return -a;
}

__device__ DecInt dec_gpu_entry(DecContext* c, const DecInt* args, int max_depth) {
    if (!dec_gpu_tick(c)) return 0;
    if (max_depth < 1) { c->error = 2; c->line = 0; return 0; }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v1 = 0LL;
    if (!v1) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v2 = 0LL;
        DecInt v3 = args[0] == v2;
        v1 = v3;
    }
    if (!v1) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v4 = 1LL;
        DecInt v5 = args[0] == v4;
        v1 = v5;
    }
    if (!v1) { c->error = 3; c->line = 5; return 0; }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v6 = 0LL;
    if (!v6) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v7 = 0LL;
        DecInt v8 = args[1] == v7;
        v6 = v8;
    }
    if (!v6) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v9 = 1LL;
        DecInt v10 = args[1] == v9;
        v6 = v10;
    }
    if (!v6) { c->error = 3; c->line = 6; return 0; }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v11 = 0LL;
    if (!v11) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v12 = 0LL;
        DecInt v13 = args[2] == v12;
        v11 = v13;
    }
    if (!v11) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v14 = 1LL;
        DecInt v15 = args[2] == v14;
        v11 = v15;
    }
    if (!v11) { c->error = 3; c->line = 7; return 0; }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v16 = 0LL;
    if (!v16) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v17 = 0LL;
        DecInt v18 = args[3] == v17;
        v16 = v18;
    }
    if (!v16) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v19 = 1LL;
        DecInt v20 = args[3] == v19;
        v16 = v20;
    }
    if (!v16) { c->error = 3; c->line = 8; return 0; }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v21 = 1LL;
    if (v21) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v22 = 0LL;
        DecInt v23 = args[4] >= v22;
        v21 = v23;
    }
    if (v21) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v24 = 2LL;
        DecInt v25 = args[4] <= v24;
        v21 = v25;
    }
    if (!v21) { c->error = 3; c->line = 9; return 0; }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v26 = dec_gpu_sub(c,args[1],args[2],10);
    if (c->error) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v27 = 1LL;
    if (v27) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v28 = 1LL;
        DecInt v29 = args[3] == v28;
        v27 = v29;
    }
    if (v27) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v30 = 0LL;
        DecInt v31 = v26 != v30;
        v27 = v31;
    }
    DecInt v32 = 0LL;
    if (v27) {
        if (!dec_gpu_tick(c)) return 0;
        DecInt v33 = 1LL;
        v32 = v33;
    } else {
        if (!dec_gpu_tick(c)) return 0;
        DecInt v34 = 0LL;
        v32 = v34;
    }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v35 = 1LL;
    if (v35) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v36 = 1LL;
        DecInt v37 = args[1] == v36;
        v35 = v37;
    }
    if (v35) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v38 = 1LL;
        DecInt v39 = args[2] == v38;
        v35 = v39;
    }
    DecInt v40 = 0LL;
    if (v35) {
        if (!dec_gpu_tick(c)) return 0;
        DecInt v41 = 1LL;
        v40 = v41;
    } else {
        if (!dec_gpu_tick(c)) return 0;
        DecInt v42 = 0LL;
        v40 = v42;
    }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v43 = 1LL;
    if (v43) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v44 = 0LL;
        DecInt v45 = args[1] == v44;
        v43 = v45;
    }
    if (v43) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v46 = 0LL;
        DecInt v47 = args[2] == v46;
        v43 = v47;
    }
    DecInt v48 = 0LL;
    if (v43) {
        if (!dec_gpu_tick(c)) return 0;
        DecInt v49 = 1LL;
        v48 = v49;
    } else {
        if (!dec_gpu_tick(c)) return 0;
        DecInt v50 = 0LL;
        v48 = v50;
    }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v51 = 1LL;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v52 = dec_gpu_sub(c,v51,args[3],14);
    if (c->error) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v53 = 1LL;
    DecInt v54 = v32 == v53;
    DecInt v55 = 0LL;
    if (v54) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v56 = 1LL;
        DecInt v57 = dec_gpu_add(c,args[4],v56,15);
        if (c->error) return 0;
        v55 = v57;
    } else {
        if (!dec_gpu_tick(c)) return 0;
        DecInt v58 = 0LL;
        v55 = v58;
    }
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    if (!dec_gpu_tick(c)) return 0;
    DecInt v59 = 0LL;
    DecInt v60 = args[0] == v59;
    DecInt v61 = 0LL;
    if (v60) {
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v62 = 1LL;
        DecInt v63 = dec_gpu_add(c,v26,v62,16);
        if (c->error) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v64 = 3LL;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v65 = dec_gpu_mul(c,v64,v32,16);
        if (c->error) return 0;
        DecInt v66 = dec_gpu_add(c,v63,v65,16);
        if (c->error) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v67 = 6LL;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v68 = dec_gpu_mul(c,v67,v40,16);
        if (c->error) return 0;
        DecInt v69 = dec_gpu_add(c,v66,v68,16);
        if (c->error) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v70 = 12LL;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v71 = dec_gpu_mul(c,v70,v48,16);
        if (c->error) return 0;
        DecInt v72 = dec_gpu_add(c,v69,v71,16);
        if (c->error) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v73 = 24LL;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v74 = dec_gpu_mul(c,v73,v52,16);
        if (c->error) return 0;
        DecInt v75 = dec_gpu_add(c,v72,v74,16);
        if (c->error) return 0;
        if (!dec_gpu_tick(c)) return 0;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v76 = 48LL;
        if (!dec_gpu_tick(c)) return 0;
        DecInt v77 = dec_gpu_mul(c,v76,v55,16);
        if (c->error) return 0;
        DecInt v78 = dec_gpu_add(c,v75,v77,16);
        if (c->error) return 0;
        v61 = v78;
    } else {
        if (!dec_gpu_tick(c)) return 0;
        v61 = v26;
    }
    return v61;
}
extern "C" __global__ void dec_gpu_rows(
    const DecInt* inputs, DecInt* values, int* errors, int* lines, DecInt* remaining,
    unsigned int count, DecInt fuel, int max_depth) {
    unsigned int row = blockIdx.x*blockDim.x+threadIdx.x;
    if (row >= count) return;
    DecContext context = {fuel,0,0};
    DecInt value = dec_gpu_entry(&context, inputs+(unsigned long long)row*5, max_depth);
    values[row] = context.error ? 0 : value;
    errors[row] = context.error; lines[row] = context.line; remaining[row] = context.fuel;
}
