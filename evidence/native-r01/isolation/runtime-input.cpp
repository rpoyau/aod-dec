#include "runtime.hpp"
#include <cstdio>
#include <cstring>
extern "C" void* dec_square__int(void*,void*);
int main(int argc,char** argv){
  if(argc!=2)return 2;
  void* c=dec_context_new(100000,128);if(!c)return 3;
  void* a=dec_literal(c,"int",argv[1],std::strlen(argv[1]),0);
  void* r=dec_square__int(c,a);
  if(!dec_ok(c)){std::fprintf(stderr,"%s\n",dec_error(c));dec_context_free(c);return 1;}
  const char* p=dec_render(c,r);std::fwrite(p,1,dec_render_size(c),stdout);std::fputc('\n',stdout);
  dec_context_free(c);return 0;
}
