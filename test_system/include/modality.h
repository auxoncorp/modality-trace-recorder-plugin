#ifndef MODALITY_H
#define MODALITY_H

#ifdef __cplusplus
extern "C" {
#endif

void modality_trace_startup_nonce(void);
void modality_announce_mutator(void);
uint32_t modality_get_and_clear_mutation(void);

#ifdef __cplusplus
}
#endif

#endif /* MODALITY_H */
