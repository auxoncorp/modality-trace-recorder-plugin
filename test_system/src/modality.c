/* Modality utilities */

#include "FreeRTOS.h"

#include <stdio.h>
#include <string.h> /* memcpy */

#include "app_config.h"
#include "logging.h"
#include "modality.h"

#define CUSTOM_EVENT_BASE (0x0FF0)
#define EVENT_MUTATOR_ANNOUNCED (CUSTOM_EVENT_BASE + 0) /* Maps to modality.mutator.announced */
#define EVENT_MUTATOR_RETIRED (CUSTOM_EVENT_BASE + 1) /* Maps to modality.mutator.retired */
#define EVENT_MUTATION_CMD_COMM (CUSTOM_EVENT_BASE + 2) /* Maps to modality.mutation.command_communicated */
#define EVENT_MUTATION_CLR_COMM (CUSTOM_EVENT_BASE + 3) /* Maps to modality.mutation.clear_communicated */
#define EVENT_MUTATION_TRIGGERED (CUSTOM_EVENT_BASE + 4) /* Maps to modality.mutation.triggered */
#define EVENT_MUTATION_INJECTED (CUSTOM_EVENT_BASE + 5) /* Maps to modality.mutation.injected */

typedef struct
{
    uint8_t mutator_id[16]; /* Mutator ID UUID bytes */
} deviant_mutator_data_s;

typedef struct
{
    uint8_t mutator_id[16]; /* Mutator ID UUID bytes */
    uint8_t mutation_id[16]; /* Mutation ID UUID bytes */
    uint32_t success; /* Status success boolean */
} deviant_mutation_data_s;

/* NOTE: zero means no-set, the test framework must write a value here regardless of use */
static __attribute__((section(".noinit"))) volatile uint32_t g_startup_nonce;
static __attribute__((section(".noinit"))) volatile uint32_t g_mutation_staged;

/* Mutator/mutation ID UUID bytes, set by the test framework */
static __attribute__((section(".noinit"))) volatile char g_mutator_id[16];
static __attribute__((section(".noinit"))) volatile char g_mutation_id[16];

static TraceStringHandle_t g_startup_ch = 0;

void modality_trace_startup_nonce(void)
{
    traceResult tr;

    if(g_startup_nonce != 0)
    {
        tr = xTraceStringRegister("test_framework_nonce", &g_startup_ch);
        configASSERT(tr == TRC_SUCCESS);

        xTracePrintF(g_startup_ch, "%u", g_startup_nonce);

        g_startup_nonce = 0;
    }
}

uint32_t modality_get_and_clear_mutation(void)
{
    const uint32_t mutation_was_staged = g_mutation_staged;
    traceResult tr;
    TraceEventHandle_t event;
    deviant_mutation_data_s data;

    configASSERT(sizeof(data) % sizeof(TraceUnsignedBaseType_t) == 0);

    if(g_mutation_staged != 0)
    {
        printf("INJECT\n");
        (void) memcpy(&data.mutator_id[0], (void*) &g_mutator_id[0], 16);
        (void) memcpy(&data.mutation_id[0], (void*) &g_mutation_id[0], 16);
        data.success = 1;

        tr = xTraceEventBeginOffline(EVENT_MUTATION_CMD_COMM, sizeof(data), &event);
        configASSERT(tr == TRC_SUCCESS);
        tr = xTraceEventAddData(
                event,
                (TraceUnsignedBaseType_t*) &data,
                sizeof(data) / sizeof(TraceUnsignedBaseType_t));
        configASSERT(tr == TRC_SUCCESS);
        tr = xTraceEventEndOffline(event);
        configASSERT(tr == TRC_SUCCESS);

        tr = xTraceEventBeginOffline(EVENT_MUTATION_INJECTED, sizeof(data), &event);
        configASSERT(tr == TRC_SUCCESS);
        tr = xTraceEventAddData(
                event,
                (TraceUnsignedBaseType_t*) &data,
                sizeof(data) / sizeof(TraceUnsignedBaseType_t));
        configASSERT(tr == TRC_SUCCESS);
        tr = xTraceEventEndOffline(event);
        configASSERT(tr == TRC_SUCCESS);

        g_mutation_staged = 0;
    }

    return mutation_was_staged;
}
