// Native threads invoke generated LLVM entries. No interpreter or Python callback.
#include "runtime.hpp"
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
#include <sys/syscall.h>
#include <unistd.h>

extern "C" {
struct DecBatchArgument { const char* text; uint64_t size; int32_t kind; };
struct DecBatchRow {
    const char* result; uint64_t result_size;
    const char* output; uint64_t output_size;
    const char* error; uint64_t error_size;
    int32_t error_line; int32_t exit_code;
    uint64_t worker_id; int32_t dispatch; int32_t entered;
};
}

namespace {
using Entry = void*(*)(void*,void**,uint64_t,int32_t*);
struct Row {
    std::string result, output, error;
    int32_t error_line=0, exit_code=1, dispatch=-1;
    uint64_t worker_id=0;
    bool entered=false;
};
struct Batch {
    std::vector<Row> rows;
    std::string error;
    std::mutex error_lock;
    std::atomic<bool> failed{false};
    std::atomic<uint64_t> output_bytes{0};
    explicit Batch(uint64_t count): rows(count) {}
    void fail(const char* message) noexcept {
        try {
            std::lock_guard<std::mutex> lock(error_lock);
            if (!failed.load()) error=message;
            failed.store(true);
        } catch (...) {
            // Allocation failure while reporting another failure must never
            // escape a worker thread. dec_batch_error supplies fixed fallback.
            failed.store(true);
        }
    }
};
struct ContextDeleter { void operator()(void* pointer) const { dec_context_free(pointer); } };

void run_lane(Batch& batch, uint64_t index, Entry entry, uint64_t identity,
              const DecBatchArgument* args, uint32_t count, int64_t fuel,
              int32_t max_depth, uint64_t output_cap) {
    Row& row=batch.rows[index];
    row.worker_id=static_cast<uint64_t>(syscall(SYS_gettid));
    std::unique_ptr<void,ContextDeleter> owner(dec_context_new(fuel,max_depth));
    void* context=owner.get();
    if (!context) throw std::runtime_error("native batch context allocation failed");
    void* handles[16]={};
    for (uint32_t column=0; column<count && dec_ok(context); ++column) {
        const auto& argument=args[index*count+column];
        handles[column]=dec_literal(context,argument.kind==0?"int":"bool",
                                    argument.text,argument.size,0);
    }
    void* result=nullptr;
    if (dec_ok(context)) {
        row.entered=true;
        result=entry(context,handles,identity,&row.dispatch);
    }
    const char* rendered=nullptr;
    uint64_t result_size=0;
    if (dec_ok(context)) {
        rendered=dec_render(context,result);
        result_size=dec_render_size(context);
    }
    bool okay=dec_ok(context);
    const char* error=okay?"":dec_error(context);
    uint64_t error_size=std::char_traits<char>::length(error);
    uint64_t output_size=dec_output_size(context);
    if (result_size>output_cap || output_size>output_cap || error_size>output_cap ||
        result_size+output_size>output_cap || result_size+output_size+error_size>output_cap)
        throw std::runtime_error("native batch output byte budget exceeded");
    const uint64_t bytes=result_size+output_size+error_size;
    uint64_t used=batch.output_bytes.load();
    do {
        if (used>output_cap-bytes) throw std::runtime_error("native batch output byte budget exceeded");
    } while (!batch.output_bytes.compare_exchange_weak(used,used+bytes));
    if (okay) row.result.assign(rendered,result_size);
    row.output.assign(dec_output(context),output_size);
    row.error.assign(error,error_size);
    row.error_line=okay?0:dec_error_line(context);
    row.exit_code=okay?0:1;
}
}

extern "C" {
void* dec_batch_execute(void* code, uint64_t identity, const DecBatchArgument* arguments,
                       uint64_t rows, uint32_t columns, uint32_t workers,
                       int64_t fuel, int32_t max_depth, uint64_t output_cap) noexcept {
    Batch* batch=nullptr;
    try {
        if (!code || rows>65536 || columns>16 || workers<1 || workers>32 ||
            fuel<0 || fuel>1000000 || max_depth<0 || max_depth>1024 ||
            !output_cap || output_cap>64*1024*1024 || (rows && columns && !arguments))
            return nullptr;
        batch=new Batch(rows);
        for (uint64_t i=0; i<rows*columns; ++i)
            if (!arguments[i].text || arguments[i].size>4096 || arguments[i].kind<0 || arguments[i].kind>1)
                throw std::runtime_error("invalid native batch argument envelope");
        Entry entry=reinterpret_cast<Entry>(code);
        std::vector<std::thread> threads;
        const uint64_t width=std::min<uint64_t>(workers,rows);
        threads.reserve(width);
        try {
            for (uint64_t worker=0; worker<width; ++worker) {
                const uint64_t begin=rows*worker/width, end=rows*(worker+1)/width;
                threads.emplace_back([=] {
                    try {
                        for (uint64_t row=begin; row<end && !batch->failed.load(); ++row)
                            run_lane(*batch,row,entry,identity,arguments,columns,fuel,max_depth,output_cap);
                    } catch (const std::exception& error) { batch->fail(error.what()); }
                      catch (...) { batch->fail("native batch worker failed"); }
                });
            }
        } catch (...) {
            batch->fail("native batch thread creation failed");
            for (auto& thread:threads) if (thread.joinable()) thread.join();
            throw;
        }
        for (auto& thread:threads) if (thread.joinable()) thread.join();
        return batch;
    } catch (const std::exception& error) {
        if (batch) { try { batch->fail(error.what()); } catch (...) {} }
        return batch;
    } catch (...) {
        if (batch) { try { batch->fail("native batch dispatch failed"); } catch (...) {} }
        return batch;
    }
}

const char* dec_batch_error(void* pointer) noexcept {
    if (!pointer) return "native batch dispatch allocation or argument failure";
    auto* batch=static_cast<Batch*>(pointer);
    if (batch->failed.load() && batch->error.empty()) return "native batch allocation or worker failure";
    return batch->error.c_str();
}

int32_t dec_batch_get(void* pointer,uint64_t index,DecBatchRow* result) noexcept {
    if (!pointer || !result) return 0;
    auto* batch=static_cast<Batch*>(pointer);
    if (batch->failed.load() || index>=batch->rows.size()) return 0;
    const auto& row=batch->rows[index];
    *result={row.exit_code?nullptr:row.result.c_str(),row.result.size(),
             row.output.c_str(),row.output.size(),row.error.c_str(),row.error.size(),
             row.error_line,row.exit_code,row.worker_id,row.dispatch,row.entered?1:0};
    return 1;
}

void dec_batch_free(void* pointer) noexcept { delete static_cast<Batch*>(pointer); }
}
