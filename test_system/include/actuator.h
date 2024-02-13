#ifndef ACTUATOR_H
#define ACTUATOR_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void actuator_init(void);

void actuator_send_adc_data(int16_t adc_value);

#ifdef __cplusplus
}
#endif

#endif /* ACTUATOR_H */
