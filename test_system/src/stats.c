/* System stats task */

#include "FreeRTOS.h"
#include "task.h"

#include "FreeRTOS_IP.h"

//#define PRINT_STATS
#ifdef PRINT_STATS
#include <stdio.h>
#endif
#include <string.h>

#include "app_config.h"
#include "logging.h"
#include "stats.h"

#define STATS_INTERVAL pdMS_TO_TICKS(500)
#define MAX_TASKS (12)

typedef struct
{
    TaskHandle_t task;
    TraceStringHandle_t sym;
    uint32_t stack_size;
    unsigned long last_runtime_counter;
} task_state_s;

static void stats_task(void* params);
static void log_task_stats(const TaskStatus_t* status, const task_state_s* state, unsigned long total_runtime);
static task_state_s* task_state(const TaskStatus_t* s);
static uint32_t task_stack_size(const char* name);

static TraceStringHandle_t g_stats_ch = NULL;
static task_state_s g_task_state[MAX_TASKS] = {0};
static TaskStatus_t g_task_stats[MAX_TASKS] = {0};
static unsigned long g_last_total_runtime = 0;

void stats_init(void)
{
    xTaskCreate(stats_task, STATS_NAME, STATS_STACK_SIZE, NULL, STATS_PRIO, NULL);
}

static void stats_task(void* params)
{
    UBaseType_t i;
    traceResult tr;
    TickType_t next_wake;
    UBaseType_t num_tasks;
    unsigned long total_runtime;
    (void) params;

    tr = xTraceStringRegister("stats", &g_stats_ch);
    configASSERT(tr == TRC_SUCCESS);

    next_wake = xTaskGetTickCount();
    while(1)
    {
        const BaseType_t was_delayed = xTaskDelayUntil(&next_wake, STATS_INTERVAL);
        if(was_delayed == pdFALSE)
        {
            WARN("Deadline missed");
        }

        num_tasks = uxTaskGetNumberOfTasks();
        configASSERT(num_tasks <= MAX_TASKS);

        num_tasks = uxTaskGetSystemState(g_task_stats, num_tasks, &total_runtime);
        configASSERT(num_tasks != 0);

        for(i = 0; i < num_tasks; i += 1)
        {
            task_state_s* state = task_state(&g_task_stats[i]);

            if((total_runtime / 100) != 0)
            {
                log_task_stats(&g_task_stats[i], state, total_runtime);
            }

            state->last_runtime_counter = g_task_stats[i].ulRunTimeCounter;
        }

        g_last_total_runtime = total_runtime;
    }
}

static void log_task_stats(const TaskStatus_t* status, const task_state_s* state, unsigned long total_runtime)
{
    traceResult tr;

#ifdef PRINT_STATS
    unsigned long total_runtime_percentage;
    total_runtime_percentage = (status->ulRunTimeCounter - state->last_runtime_counter) / ((total_runtime - g_last_total_runtime) / 100);
    /* If the percentage is zero here then the task has
    consumed less than 1% of the total run time. */
    if(total_runtime_percentage == 0)
    {
        total_runtime_percentage = 1;
    }

    printf("[%s] %lu %d %lu %lu%%\n",
            status->pcTaskName,
            state->stack_size,
            status->usStackHighWaterMark,
            status->ulRunTimeCounter - state->last_runtime_counter,
            total_runtime_percentage);
#endif

    tr = xTracePrintF(
            g_stats_ch,
            "%s %u %d %u %u",
            state->sym,
            state->stack_size,
            status->usStackHighWaterMark,
            status->ulRunTimeCounter - state->last_runtime_counter,
            total_runtime - g_last_total_runtime);
    configASSERT(tr == TRC_SUCCESS);
}

static task_state_s* task_state(const TaskStatus_t* s)
{
    int i;
    traceResult tr;
    task_state_s* state = NULL;

    // Check if already registered
    for(i = 0; i < MAX_TASKS; i += 1)
    {
        if(g_task_state[i].task == s->xHandle)
        {
            state = &g_task_state[i];
            break;
        }
    }

    // Allocate a new entry
    if(state == NULL)
    {
        for(i = 0; i < MAX_TASKS; i += 1)
        {
            if(g_task_state[i].task == NULL)
            {
                configASSERT(g_task_state[i].sym == NULL);
                tr = xTraceStringRegister(s->pcTaskName, &g_task_state[i].sym);
                configASSERT(tr == TRC_SUCCESS);
                g_task_state[i].task = s->xHandle;
                g_task_state[i].stack_size = task_stack_size(s->pcTaskName);
                state = &g_task_state[i];
                break;
            }
        }
    }

    configASSERT(state != NULL);

    return state;
}

static uint32_t task_stack_size(const char* name)
{
    uint32_t stack_size = 0;

    if(strncmp(name, STATS_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = STATS_STACK_SIZE;
    }
    else if(strncmp(name, SENSOR_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = SENSOR_STACK_SIZE;
    }
    else if(strncmp(name, ACTUATOR_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = ACTUATOR_STACK_SIZE;
    }
    else if(strncmp(name, COMMS_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = COMMS_STACK_SIZE;
    }
    else if(strncmp(name, IDLE_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = IDLE_STACK_SIZE;
    }
    else if(strncmp(name, TIMER_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = TIMER_STACK_SIZE;
    }
    else if(strncmp(name, TZCTRL_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = TZCTRL_STACK_SIZE;
    }
    else if(strncmp(name, IP_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = IP_STACK_SIZE;
    }
    else if(strncmp(name, EMAC_NAME, configMAX_TASK_NAME_LEN) == 0)
    {
        stack_size = EMAC_STACK_SIZE;
    }

    configASSERT(stack_size != 0);
    return stack_size;
}
