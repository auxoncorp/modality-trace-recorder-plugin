from Antmicro import Renode
import uuid

def mc_clear_modality_noint_vars():
    sysbus = self.Machine["sysbus"]
    var_startup_nonce_addr = sysbus.GetSymbolAddress("g_startup_nonce")
    sysbus.WriteDoubleWord(var_startup_nonce_addr, 0)
    var_mutation_staged_addr = sysbus.GetSymbolAddress("g_mutation_staged")
    sysbus.WriteDoubleWord(var_mutation_staged_addr, 0)

def mc_write_staged_mutation(mutator_uuid_str, mutation_uuid_str):
    print("Writing staged mutation, mutator_id = %s, mutation_id = %s" % (mutator_uuid_str, mutation_uuid_str))
    mutator_uuid = uuid.UUID(mutator_uuid_str)
    mutation_uuid = uuid.UUID(mutation_uuid_str)

    sysbus = self.Machine["sysbus"]
    var_mutation_staged_addr = sysbus.GetSymbolAddress("g_mutation_staged")
    var_mutator_id_addr = sysbus.GetSymbolAddress("g_mutator_id");
    var_mutation_id_addr = sysbus.GetSymbolAddress("g_mutation_id");

    for offset, b in enumerate(mutator_uuid.bytes):
        val = int(b.encode('hex'), 16)
        sysbus.WriteByte(var_mutator_id_addr + offset, val)
    for offset, b in enumerate(mutation_uuid.bytes):
        val = int(b.encode('hex'), 16)
        sysbus.WriteByte(var_mutation_id_addr + offset, val)

    sysbus.WriteDoubleWord(var_mutation_staged_addr, 1)
