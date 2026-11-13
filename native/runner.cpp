#include "runtime.hpp"
#include <cstdio>
#include <cstdlib>
#include <cstring>
extern "C" void* dec_main__void(void*);
int main(int argc,char** argv) {
    long long fuel=100000; int depth=128;
    if (argc>1) fuel=std::strtoll(argv[1],nullptr,10);
    if (argc>2) depth=std::atoi(argv[2]);
    void* ctx=dec_context_new(fuel,depth);
    if(!ctx){std::fputs("aodc: native context allocation failed\n",stderr);return 1;}
    void* result=dec_main__void(ctx);
    std::fwrite(dec_output(ctx),1,dec_output_size(ctx),stdout);
    int status=0;
    if(!dec_ok(ctx)){std::fprintf(stderr,"aodc: %s\n",dec_error(ctx));status=1;}
    else {const char* text=dec_render(ctx,result); std::fwrite(text,1,dec_render_size(ctx),stdout);std::fputc('\n',stdout);}
    dec_context_free(ctx); return status;
}
