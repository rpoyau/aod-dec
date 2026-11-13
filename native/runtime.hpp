#ifndef DEC_RUNTIME_HPP
#define DEC_RUNTIME_HPP
#include <cstdint>
extern "C" {
void* dec_context_new(int64_t fuel,int32_t depth_limit);
void dec_context_free(void*);
int32_t dec_ok(void*);
const char* dec_error(void*);
int32_t dec_error_line(void*);
int32_t dec_tick(void*);
int32_t dec_enter(void*);
void dec_leave(void*);
void* dec_literal(void*,const char* type,const char* text,uint64_t length,int32_t line);
void* dec_unary(void*,const char* op,void* value,int32_t line);
void* dec_binary(void*,const char* op,void* left,void* right,int32_t line);
void* dec_builtin(void*,const char* name,void** args,int32_t count,int32_t line);
int32_t dec_truth(void*,void* value);
void dec_emit(void*,void*,int32_t line);
void dec_assert(void*,void*,int32_t line);
const char* dec_render(void*,void*);
uint64_t dec_render_size(void*);
const char* dec_output(void*);
uint64_t dec_output_size(void*);
void dec_set_allocation_limit(void*,uint64_t);
}
#endif
