#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CFPlugInCOM.h>
#include <stdatomic.h>
#include <string.h>

static atomic_uint g_ref_count = 1;
static AudioServerPlugInHostRef g_host = NULL;

static HRESULT STDMETHODCALLTYPE rusty_jack_query_interface(void *driver, REFIID uuid, LPVOID *out_interface);
static ULONG STDMETHODCALLTYPE rusty_jack_add_ref(void *driver);
static ULONG STDMETHODCALLTYPE rusty_jack_release(void *driver);
static OSStatus STDMETHODCALLTYPE rusty_jack_initialize(AudioServerPlugInDriverRef driver, AudioServerPlugInHostRef host);
static OSStatus STDMETHODCALLTYPE rusty_jack_create_device(AudioServerPlugInDriverRef driver, CFDictionaryRef description, const AudioServerPlugInClientInfo *client_info, AudioObjectID *device_object_id);
static OSStatus STDMETHODCALLTYPE rusty_jack_destroy_device(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id);
static OSStatus STDMETHODCALLTYPE rusty_jack_add_device_client(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, const AudioServerPlugInClientInfo *client_info);
static OSStatus STDMETHODCALLTYPE rusty_jack_remove_device_client(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, const AudioServerPlugInClientInfo *client_info);
static OSStatus STDMETHODCALLTYPE rusty_jack_perform_device_configuration_change(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt64 change_action, void *change_info);
static OSStatus STDMETHODCALLTYPE rusty_jack_abort_device_configuration_change(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt64 change_action, void *change_info);
static Boolean STDMETHODCALLTYPE rusty_jack_has_property(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address);
static OSStatus STDMETHODCALLTYPE rusty_jack_is_property_settable(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, Boolean *is_settable);
static OSStatus STDMETHODCALLTYPE rusty_jack_get_property_data_size(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 *data_size);
static OSStatus STDMETHODCALLTYPE rusty_jack_get_property_data(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 data_size, UInt32 *out_data_size, void *out_data);
static OSStatus STDMETHODCALLTYPE rusty_jack_set_property_data(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 data_size, const void *data);
static OSStatus STDMETHODCALLTYPE rusty_jack_start_io(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id);
static OSStatus STDMETHODCALLTYPE rusty_jack_stop_io(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id);
static OSStatus STDMETHODCALLTYPE rusty_jack_get_zero_time_stamp(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, Float64 *sample_time, UInt64 *host_time, UInt64 *seed);
static OSStatus STDMETHODCALLTYPE rusty_jack_will_do_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, Boolean *will_do, Boolean *will_do_in_place);
static OSStatus STDMETHODCALLTYPE rusty_jack_begin_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info);
static OSStatus STDMETHODCALLTYPE rusty_jack_do_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, AudioObjectID stream_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info, void *main_buffer, void *secondary_buffer);
static OSStatus STDMETHODCALLTYPE rusty_jack_end_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info);

static AudioServerPlugInDriverInterface g_driver_interface = {
    NULL,
    rusty_jack_query_interface,
    rusty_jack_add_ref,
    rusty_jack_release,
    rusty_jack_initialize,
    rusty_jack_create_device,
    rusty_jack_destroy_device,
    rusty_jack_add_device_client,
    rusty_jack_remove_device_client,
    rusty_jack_perform_device_configuration_change,
    rusty_jack_abort_device_configuration_change,
    rusty_jack_has_property,
    rusty_jack_is_property_settable,
    rusty_jack_get_property_data_size,
    rusty_jack_get_property_data,
    rusty_jack_set_property_data,
    rusty_jack_start_io,
    rusty_jack_stop_io,
    rusty_jack_get_zero_time_stamp,
    rusty_jack_will_do_io_operation,
    rusty_jack_begin_io_operation,
    rusty_jack_do_io_operation,
    rusty_jack_end_io_operation,
};

static AudioServerPlugInDriverInterface *g_driver_interface_ptr = &g_driver_interface;

static Boolean uuid_ref_equals(CFUUIDRef left, CFUUIDRef right) {
    CFUUIDBytes left_bytes = CFUUIDGetUUIDBytes(left);
    CFUUIDBytes right_bytes = CFUUIDGetUUIDBytes(right);
    return memcmp(&left_bytes, &right_bytes, sizeof(CFUUIDBytes)) == 0;
}

static Boolean uuid_bytes_equals_ref(REFIID left, CFUUIDRef right) {
    CFUUIDBytes right_bytes = CFUUIDGetUUIDBytes(right);
    return memcmp(&left, &right_bytes, sizeof(CFUUIDBytes)) == 0;
}

__attribute__((visibility("default"))) void *RustyJackDriverFactory(CFAllocatorRef allocator, CFUUIDRef type_uuid) {
    (void)allocator;
    if (!uuid_ref_equals(type_uuid, kAudioServerPlugInTypeUUID)) {
        return NULL;
    }

    rusty_jack_add_ref(&g_driver_interface_ptr);
    return &g_driver_interface_ptr;
}

static HRESULT STDMETHODCALLTYPE rusty_jack_query_interface(void *driver, REFIID uuid, LPVOID *out_interface) {
    (void)driver;
    if (out_interface == NULL) {
        return E_POINTER;
    }

    if (uuid_bytes_equals_ref(uuid, IUnknownUUID) || uuid_bytes_equals_ref(uuid, kAudioServerPlugInDriverInterfaceUUID)) {
        rusty_jack_add_ref(&g_driver_interface_ptr);
        *out_interface = &g_driver_interface_ptr;
        return S_OK;
    }

    *out_interface = NULL;
    return E_NOINTERFACE;
}

static ULONG STDMETHODCALLTYPE rusty_jack_add_ref(void *driver) {
    (void)driver;
    return atomic_fetch_add_explicit(&g_ref_count, 1, memory_order_relaxed) + 1;
}

static ULONG STDMETHODCALLTYPE rusty_jack_release(void *driver) {
    (void)driver;
    unsigned int current = atomic_load_explicit(&g_ref_count, memory_order_relaxed);
    while (current > 1) {
        if (atomic_compare_exchange_weak_explicit(&g_ref_count, &current, current - 1, memory_order_relaxed, memory_order_relaxed)) {
            return current - 1;
        }
    }
    return current;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_initialize(AudioServerPlugInDriverRef driver, AudioServerPlugInHostRef host) {
    (void)driver;
    g_host = host;
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_create_device(AudioServerPlugInDriverRef driver, CFDictionaryRef description, const AudioServerPlugInClientInfo *client_info, AudioObjectID *device_object_id) {
    (void)driver;
    (void)description;
    (void)client_info;
    if (device_object_id != NULL) {
        *device_object_id = kAudioObjectUnknown;
    }
    return kAudioHardwareIllegalOperationError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_destroy_device(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id) {
    (void)driver;
    (void)device_object_id;
    return kAudioHardwareIllegalOperationError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_add_device_client(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, const AudioServerPlugInClientInfo *client_info) {
    (void)driver;
    (void)device_object_id;
    (void)client_info;
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_remove_device_client(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, const AudioServerPlugInClientInfo *client_info) {
    (void)driver;
    (void)device_object_id;
    (void)client_info;
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_perform_device_configuration_change(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt64 change_action, void *change_info) {
    (void)driver;
    (void)device_object_id;
    (void)change_action;
    (void)change_info;
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_abort_device_configuration_change(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt64 change_action, void *change_info) {
    (void)driver;
    (void)device_object_id;
    (void)change_action;
    (void)change_info;
    return noErr;
}

static Boolean is_plugin_object(AudioObjectID object_id) {
    return object_id == kAudioObjectPlugInObject;
}

static Boolean is_plugin_property(AudioObjectPropertySelector selector) {
    switch (selector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioObjectPropertyOwnedObjects:
            return true;
        default:
            return false;
    }
}

static Boolean STDMETHODCALLTYPE rusty_jack_has_property(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address) {
    (void)driver;
    (void)client_process_id;
    return address != NULL && is_plugin_object(object_id) && is_plugin_property(address->mSelector);
}

static OSStatus STDMETHODCALLTYPE rusty_jack_is_property_settable(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, Boolean *is_settable) {
    (void)driver;
    (void)client_process_id;
    if (is_settable == NULL) {
        return kAudioHardwareBadPropertySizeError;
    }
    if (!rusty_jack_has_property(driver, object_id, client_process_id, address)) {
        return kAudioHardwareUnknownPropertyError;
    }
    *is_settable = false;
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_get_property_data_size(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 *data_size) {
    (void)qualifier_data_size;
    (void)qualifier_data;
    if (data_size == NULL) {
        return kAudioHardwareBadPropertySizeError;
    }
    if (!rusty_jack_has_property(driver, object_id, client_process_id, address)) {
        return kAudioHardwareUnknownPropertyError;
    }

    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
            *data_size = sizeof(UInt32);
            return noErr;
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
            *data_size = sizeof(CFStringRef);
            return noErr;
        case kAudioObjectPropertyOwnedObjects:
            *data_size = 0;
            return noErr;
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus write_u32(UInt32 value, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    if (out_data_size == NULL || out_data == NULL || data_size < sizeof(UInt32)) {
        return kAudioHardwareBadPropertySizeError;
    }
    *((UInt32 *)out_data) = value;
    *out_data_size = sizeof(UInt32);
    return noErr;
}

static OSStatus write_cf_string(CFStringRef value, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    if (out_data_size == NULL || out_data == NULL || data_size < sizeof(CFStringRef)) {
        return kAudioHardwareBadPropertySizeError;
    }
    *((CFStringRef *)out_data) = (CFStringRef)CFRetain(value);
    *out_data_size = sizeof(CFStringRef);
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_get_property_data(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    (void)qualifier_data_size;
    (void)qualifier_data;
    if (!rusty_jack_has_property(driver, object_id, client_process_id, address)) {
        return kAudioHardwareUnknownPropertyError;
    }

    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            return write_u32(kAudioObjectClassID, data_size, out_data_size, out_data);
        case kAudioObjectPropertyClass:
            return write_u32(kAudioPlugInClassID, data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwner:
            return write_u32(kAudioObjectUnknown, data_size, out_data_size, out_data);
        case kAudioObjectPropertyName:
            return write_cf_string(CFSTR("Rusty Jack"), data_size, out_data_size, out_data);
        case kAudioObjectPropertyManufacturer:
            return write_cf_string(CFSTR("the-hcma"), data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwnedObjects:
            if (out_data_size == NULL) {
                return kAudioHardwareBadPropertySizeError;
            }
            *out_data_size = 0;
            return noErr;
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus STDMETHODCALLTYPE rusty_jack_set_property_data(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 data_size, const void *data) {
    (void)driver;
    (void)object_id;
    (void)client_process_id;
    (void)address;
    (void)qualifier_data_size;
    (void)qualifier_data;
    (void)data_size;
    (void)data;
    return kAudioHardwareIllegalOperationError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_start_io(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id) {
    (void)driver;
    (void)device_object_id;
    (void)client_id;
    return kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_stop_io(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id) {
    (void)driver;
    (void)device_object_id;
    (void)client_id;
    return kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_get_zero_time_stamp(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, Float64 *sample_time, UInt64 *host_time, UInt64 *seed) {
    (void)driver;
    (void)device_object_id;
    (void)client_id;
    (void)sample_time;
    (void)host_time;
    (void)seed;
    return kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_will_do_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, Boolean *will_do, Boolean *will_do_in_place) {
    (void)driver;
    (void)device_object_id;
    (void)client_id;
    (void)operation_id;
    if (will_do != NULL) {
        *will_do = false;
    }
    if (will_do_in_place != NULL) {
        *will_do_in_place = true;
    }
    return kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_begin_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info) {
    (void)driver;
    (void)device_object_id;
    (void)client_id;
    (void)operation_id;
    (void)io_buffer_frame_size;
    (void)io_cycle_info;
    return kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_do_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, AudioObjectID stream_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info, void *main_buffer, void *secondary_buffer) {
    (void)driver;
    (void)device_object_id;
    (void)stream_object_id;
    (void)client_id;
    (void)operation_id;
    (void)io_buffer_frame_size;
    (void)io_cycle_info;
    (void)main_buffer;
    (void)secondary_buffer;
    return kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_end_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info) {
    (void)driver;
    (void)device_object_id;
    (void)client_id;
    (void)operation_id;
    (void)io_buffer_frame_size;
    (void)io_cycle_info;
    return kAudioHardwareBadDeviceError;
}
