#ifndef WIRE_PROTOCOL_H
#define WIRE_PROTOCOL_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SENSOR_DATA_PORT (9889)

#define WIRE_MAGIC0 (0xAA)
#define WIRE_MAGIC1 (0xBB)

#define WIRE_TYPE_ACTUATOR_STATE (0xF0)

typedef struct
{
    uint8_t magic0; /* WIRE_MAGIC0 */
    uint8_t magic1; /* WIRE_MAGIC1 */
    uint8_t type;
    uint32_t seqnum; /* Starts at 1 */
    int16_t adc;
    int16_t pwm;
} __attribute__((packed, aligned(1))) wire_msg_s;

#define WIRE_MSG_SIZE (sizeof(wire_msg_s))

#ifdef __cplusplus
}
#endif

#endif /* WIRE_PROTOCOL_H */
