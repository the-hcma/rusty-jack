#include <CoreAudio/AudioHardware.h>
#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CFPlugInCOM.h>
#include <mach/mach_time.h>
#include <math.h>
#include <stdatomic.h>
#include <string.h>

// This driver publishes a minimal virtual output device and intentionally drops IO.
// The Rust daemon will gain passthrough rendering in a later slice.
#define RUSTY_JACK_DEVICE_OBJECT_ID 2
#define RUSTY_JACK_STREAM_OBJECT_ID 3
#define RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID 4
#define RUSTY_JACK_MUTE_CONTROL_OBJECT_ID 5

#define RUSTY_JACK_SAMPLE_RATE 48000.0
#define RUSTY_JACK_CHANNEL_COUNT 2
#define RUSTY_JACK_BUFFER_FRAME_SIZE 512
#define RUSTY_JACK_BUNDLE_ID "com.the-hcma.rusty-jack.driver"
#define RUSTY_JACK_DEVICE_UID "com.the-hcma.rusty-jack.driver.output"
#define RUSTY_JACK_MODEL_UID "com.the-hcma.rusty-jack.driver.model"

static atomic_uint g_ref_count = 1;
static AudioServerPlugInHostRef g_host = NULL;
static atomic_uint g_io_client_count = 0;
static atomic_ullong g_io_start_host_time = 0;
static Float32 g_volume_scalar = 1.0f;
static UInt32 g_muted = 0;

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

static Boolean object_exists(AudioObjectID object_id) {
    return object_id == kAudioObjectPlugInObject ||
           object_id == RUSTY_JACK_DEVICE_OBJECT_ID ||
           object_id == RUSTY_JACK_STREAM_OBJECT_ID ||
           object_id == RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID ||
           object_id == RUSTY_JACK_MUTE_CONTROL_OBJECT_ID;
}

static AudioClassID object_class(AudioObjectID object_id) {
    switch (object_id) {
        case kAudioObjectPlugInObject:
            return kAudioPlugInClassID;
        case RUSTY_JACK_DEVICE_OBJECT_ID:
            return kAudioDeviceClassID;
        case RUSTY_JACK_STREAM_OBJECT_ID:
            return kAudioStreamClassID;
        case RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID:
            return kAudioVolumeControlClassID;
        case RUSTY_JACK_MUTE_CONTROL_OBJECT_ID:
            return kAudioMuteControlClassID;
        default:
            return kAudioObjectClassID;
    }
}

static AudioClassID object_base_class(AudioObjectID object_id) {
    switch (object_id) {
        case kAudioObjectPlugInObject:
            return kAudioObjectClassID;
        case RUSTY_JACK_DEVICE_OBJECT_ID:
            return kAudioObjectClassID;
        case RUSTY_JACK_STREAM_OBJECT_ID:
            return kAudioObjectClassID;
        case RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID:
            return kAudioLevelControlClassID;
        case RUSTY_JACK_MUTE_CONTROL_OBJECT_ID:
            return kAudioBooleanControlClassID;
        default:
            return kAudioObjectClassID;
    }
}

static AudioObjectID object_owner(AudioObjectID object_id) {
    switch (object_id) {
        case kAudioObjectPlugInObject:
            return kAudioObjectUnknown;
        case RUSTY_JACK_DEVICE_OBJECT_ID:
            return kAudioObjectPlugInObject;
        case RUSTY_JACK_STREAM_OBJECT_ID:
        case RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID:
        case RUSTY_JACK_MUTE_CONTROL_OBJECT_ID:
            return RUSTY_JACK_DEVICE_OBJECT_ID;
        default:
            return kAudioObjectUnknown;
    }
}

static CFStringRef object_name(AudioObjectID object_id) {
    switch (object_id) {
        case kAudioObjectPlugInObject:
        case RUSTY_JACK_DEVICE_OBJECT_ID:
            return CFSTR("Rusty Jack");
        case RUSTY_JACK_STREAM_OBJECT_ID:
            return CFSTR("Rusty Jack Output Stream");
        case RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID:
            return CFSTR("Rusty Jack Volume");
        case RUSTY_JACK_MUTE_CONTROL_OBJECT_ID:
            return CFSTR("Rusty Jack Mute");
        default:
            return CFSTR("Rusty Jack");
    }
}

static AudioStreamBasicDescription stream_format(void) {
    AudioStreamBasicDescription format = {
        .mSampleRate = RUSTY_JACK_SAMPLE_RATE,
        .mFormatID = kAudioFormatLinearPCM,
        .mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagsNativeEndian | kAudioFormatFlagIsPacked,
        .mBytesPerPacket = sizeof(Float32) * RUSTY_JACK_CHANNEL_COUNT,
        .mFramesPerPacket = 1,
        .mBytesPerFrame = sizeof(Float32) * RUSTY_JACK_CHANNEL_COUNT,
        .mChannelsPerFrame = RUSTY_JACK_CHANNEL_COUNT,
        .mBitsPerChannel = sizeof(Float32) * 8,
        .mReserved = 0,
    };
    return format;
}

static AudioStreamRangedDescription stream_ranged_description(void) {
    AudioStreamRangedDescription description = {
        .mFormat = stream_format(),
        .mSampleRateRange = {
            .mMinimum = RUSTY_JACK_SAMPLE_RATE,
            .mMaximum = RUSTY_JACK_SAMPLE_RATE,
        },
    };
    return description;
}

static Float32 clamp_scalar(Float32 value) {
    if (value < 0.0f) {
        return 0.0f;
    }
    if (value > 1.0f) {
        return 1.0f;
    }
    return value;
}

static Float32 scalar_to_db(Float32 value) {
    value = clamp_scalar(value);
    if (value <= 0.000016f) {
        return -96.0f;
    }
    return 20.0f * log10f(value);
}

static Float32 db_to_scalar(Float32 value) {
    if (value <= -96.0f) {
        return 0.0f;
    }
    if (value >= 0.0f) {
        return 1.0f;
    }
    return clamp_scalar(powf(10.0f, value / 20.0f));
}

static Boolean plugin_has_property(AudioObjectPropertySelector selector) {
    switch (selector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioObjectPropertyOwnedObjects:
        case kAudioPlugInPropertyBundleID:
        case kAudioPlugInPropertyDeviceList:
        case kAudioPlugInPropertyTranslateUIDToDevice:
            return true;
        default:
            return false;
    }
}

static Boolean device_has_property(AudioObjectPropertySelector selector) {
    switch (selector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioObjectPropertyOwnedObjects:
        case kAudioDevicePropertyDeviceUID:
        case kAudioDevicePropertyModelUID:
        case kAudioDevicePropertyTransportType:
        case kAudioDevicePropertyDeviceIsAlive:
        case kAudioDevicePropertyDeviceIsRunning:
        case kAudioDevicePropertyDeviceCanBeDefaultDevice:
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
        case kAudioDevicePropertyLatency:
        case kAudioDevicePropertyStreams:
        case kAudioObjectPropertyControlList:
        case kAudioDevicePropertySafetyOffset:
        case kAudioDevicePropertyNominalSampleRate:
        case kAudioDevicePropertyAvailableNominalSampleRates:
        case kAudioDevicePropertyIsHidden:
        case kAudioDevicePropertyPreferredChannelsForStereo:
        case kAudioDevicePropertyBufferFrameSize:
        case kAudioDevicePropertyBufferFrameSizeRange:
        case kAudioDevicePropertyUsesVariableBufferFrameSizes:
        case kAudioDevicePropertyStreamConfiguration:
            return true;
        default:
            return false;
    }
}

static Boolean stream_has_property(AudioObjectPropertySelector selector) {
    switch (selector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioStreamPropertyIsActive:
        case kAudioStreamPropertyDirection:
        case kAudioStreamPropertyTerminalType:
        case kAudioStreamPropertyStartingChannel:
        case kAudioStreamPropertyLatency:
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyPhysicalFormat:
        case kAudioStreamPropertyAvailablePhysicalFormats:
            return true;
        default:
            return false;
    }
}

static Boolean control_has_property(AudioObjectID object_id, AudioObjectPropertySelector selector) {
    switch (selector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioControlPropertyScope:
        case kAudioControlPropertyElement:
            return true;
        case kAudioLevelControlPropertyScalarValue:
        case kAudioLevelControlPropertyDecibelValue:
        case kAudioLevelControlPropertyDecibelRange:
        case kAudioLevelControlPropertyConvertScalarToDecibels:
        case kAudioLevelControlPropertyConvertDecibelsToScalar:
            return object_id == RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID;
        case kAudioBooleanControlPropertyValue:
            return object_id == RUSTY_JACK_MUTE_CONTROL_OBJECT_ID;
        default:
            return false;
    }
}

static OSStatus data_size_for_property(AudioObjectID object_id, const AudioObjectPropertyAddress *address, UInt32 *data_size) {
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioDevicePropertyTransportType:
        case kAudioDevicePropertyDeviceIsAlive:
        case kAudioDevicePropertyDeviceIsRunning:
        case kAudioDevicePropertyDeviceCanBeDefaultDevice:
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
        case kAudioDevicePropertyLatency:
        case kAudioDevicePropertySafetyOffset:
        case kAudioDevicePropertyIsHidden:
        case kAudioDevicePropertyBufferFrameSize:
        case kAudioDevicePropertyUsesVariableBufferFrameSizes:
        case kAudioStreamPropertyIsActive:
        case kAudioStreamPropertyDirection:
        case kAudioStreamPropertyTerminalType:
        case kAudioStreamPropertyStartingChannel:
        case kAudioControlPropertyScope:
        case kAudioControlPropertyElement:
        case kAudioBooleanControlPropertyValue:
            *data_size = sizeof(UInt32);
            return noErr;
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioPlugInPropertyBundleID:
        case kAudioDevicePropertyDeviceUID:
        case kAudioDevicePropertyModelUID:
            *data_size = sizeof(CFStringRef);
            return noErr;
        case kAudioObjectPropertyOwnedObjects:
            if (object_id == kAudioObjectPlugInObject) {
                *data_size = sizeof(AudioObjectID);
            } else if (object_id == RUSTY_JACK_DEVICE_OBJECT_ID) {
                *data_size = sizeof(AudioObjectID) * 3;
            } else {
                *data_size = 0;
            }
            return noErr;
        case kAudioPlugInPropertyDeviceList:
            *data_size = sizeof(AudioObjectID);
            return noErr;
        case kAudioPlugInPropertyTranslateUIDToDevice:
            *data_size = sizeof(AudioObjectID);
            return noErr;
        case kAudioDevicePropertyStreams:
            *data_size = address->mScope == kAudioObjectPropertyScopeInput ? 0 : sizeof(AudioObjectID);
            return noErr;
        case kAudioObjectPropertyControlList:
            *data_size = sizeof(AudioObjectID) * 2;
            return noErr;
        case kAudioDevicePropertyNominalSampleRate:
            *data_size = sizeof(Float64);
            return noErr;
        case kAudioDevicePropertyAvailableNominalSampleRates:
        case kAudioDevicePropertyBufferFrameSizeRange:
        case kAudioLevelControlPropertyDecibelRange:
            *data_size = sizeof(AudioValueRange);
            return noErr;
        case kAudioDevicePropertyPreferredChannelsForStereo:
            *data_size = sizeof(UInt32) * 2;
            return noErr;
        case kAudioDevicePropertyStreamConfiguration:
            *data_size = sizeof(AudioBufferList);
            return noErr;
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat:
            *data_size = sizeof(AudioStreamBasicDescription);
            return noErr;
        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyAvailablePhysicalFormats:
            *data_size = sizeof(AudioStreamRangedDescription);
            return noErr;
        case kAudioLevelControlPropertyScalarValue:
        case kAudioLevelControlPropertyDecibelValue:
        case kAudioLevelControlPropertyConvertScalarToDecibels:
        case kAudioLevelControlPropertyConvertDecibelsToScalar:
            *data_size = sizeof(Float32);
            return noErr;
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus write_data(const void *source, UInt32 source_size, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    if (out_data_size == NULL || (source_size > 0 && out_data == NULL) || data_size < source_size) {
        return kAudioHardwareBadPropertySizeError;
    }
    if (source_size > 0) {
        memcpy(out_data, source, source_size);
    }
    *out_data_size = source_size;
    return noErr;
}

static OSStatus write_u32(UInt32 value, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    return write_data(&value, sizeof(value), data_size, out_data_size, out_data);
}

static OSStatus write_f32(Float32 value, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    return write_data(&value, sizeof(value), data_size, out_data_size, out_data);
}

static OSStatus write_f64(Float64 value, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    return write_data(&value, sizeof(value), data_size, out_data_size, out_data);
}

static OSStatus write_cf_string(CFStringRef value, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    if (out_data_size == NULL || out_data == NULL || data_size < sizeof(CFStringRef)) {
        return kAudioHardwareBadPropertySizeError;
    }
    *((CFStringRef *)out_data) = (CFStringRef)CFRetain(value);
    *out_data_size = sizeof(CFStringRef);
    return noErr;
}

static OSStatus write_object_ids(const AudioObjectID *ids, UInt32 count, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    return write_data(ids, sizeof(AudioObjectID) * count, data_size, out_data_size, out_data);
}

static OSStatus write_stream_configuration(AudioObjectPropertyScope scope, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    if (out_data_size == NULL || out_data == NULL || data_size < sizeof(AudioBufferList)) {
        return kAudioHardwareBadPropertySizeError;
    }

    AudioBufferList *buffers = (AudioBufferList *)out_data;
    buffers->mNumberBuffers = scope == kAudioObjectPropertyScopeInput ? 0 : 1;
    buffers->mBuffers[0].mNumberChannels = scope == kAudioObjectPropertyScopeInput ? 0 : RUSTY_JACK_CHANNEL_COUNT;
    buffers->mBuffers[0].mDataByteSize = 0;
    buffers->mBuffers[0].mData = NULL;
    *out_data_size = sizeof(AudioBufferList);
    return noErr;
}

static Boolean cf_string_equals_cstr(CFStringRef value, const char *expected) {
    if (value == NULL) {
        return false;
    }
    CFStringRef expected_string = CFStringCreateWithCString(NULL, expected, kCFStringEncodingUTF8);
    if (expected_string == NULL) {
        return false;
    }
    Boolean matches = CFStringCompare(value, expected_string, 0) == kCFCompareEqualTo;
    CFRelease(expected_string);
    return matches;
}

static OSStatus get_plugin_property_data(const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            return write_u32(object_base_class(kAudioObjectPlugInObject), data_size, out_data_size, out_data);
        case kAudioObjectPropertyClass:
            return write_u32(object_class(kAudioObjectPlugInObject), data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwner:
            return write_u32(object_owner(kAudioObjectPlugInObject), data_size, out_data_size, out_data);
        case kAudioObjectPropertyName:
            return write_cf_string(object_name(kAudioObjectPlugInObject), data_size, out_data_size, out_data);
        case kAudioObjectPropertyManufacturer:
            return write_cf_string(CFSTR("the-hcma"), data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwnedObjects:
        case kAudioPlugInPropertyDeviceList: {
            AudioObjectID ids[] = { RUSTY_JACK_DEVICE_OBJECT_ID };
            return write_object_ids(ids, 1, data_size, out_data_size, out_data);
        }
        case kAudioPlugInPropertyBundleID:
            return write_cf_string(CFSTR(RUSTY_JACK_BUNDLE_ID), data_size, out_data_size, out_data);
        case kAudioPlugInPropertyTranslateUIDToDevice: {
            AudioObjectID id = kAudioObjectUnknown;
            if (qualifier_data_size >= sizeof(CFStringRef) && qualifier_data != NULL) {
                CFStringRef uid = *((CFStringRef const *)qualifier_data);
                if (cf_string_equals_cstr(uid, RUSTY_JACK_DEVICE_UID)) {
                    id = RUSTY_JACK_DEVICE_OBJECT_ID;
                }
            }
            return write_u32(id, data_size, out_data_size, out_data);
        }
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus get_device_property_data(const AudioObjectPropertyAddress *address, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            return write_u32(object_base_class(RUSTY_JACK_DEVICE_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyClass:
            return write_u32(object_class(RUSTY_JACK_DEVICE_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwner:
            return write_u32(object_owner(RUSTY_JACK_DEVICE_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyName:
            return write_cf_string(object_name(RUSTY_JACK_DEVICE_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyManufacturer:
            return write_cf_string(CFSTR("the-hcma"), data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwnedObjects: {
            AudioObjectID ids[] = { RUSTY_JACK_STREAM_OBJECT_ID, RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID, RUSTY_JACK_MUTE_CONTROL_OBJECT_ID };
            return write_object_ids(ids, 3, data_size, out_data_size, out_data);
        }
        case kAudioDevicePropertyDeviceUID:
            return write_cf_string(CFSTR(RUSTY_JACK_DEVICE_UID), data_size, out_data_size, out_data);
        case kAudioDevicePropertyModelUID:
            return write_cf_string(CFSTR(RUSTY_JACK_MODEL_UID), data_size, out_data_size, out_data);
        case kAudioDevicePropertyTransportType:
            return write_u32(kAudioDeviceTransportTypeVirtual, data_size, out_data_size, out_data);
        case kAudioDevicePropertyDeviceIsAlive:
            return write_u32(1, data_size, out_data_size, out_data);
        case kAudioDevicePropertyDeviceIsRunning:
            return write_u32(atomic_load_explicit(&g_io_client_count, memory_order_relaxed) > 0 ? 1 : 0, data_size, out_data_size, out_data);
        case kAudioDevicePropertyDeviceCanBeDefaultDevice:
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
            return write_u32(address->mScope == kAudioObjectPropertyScopeInput ? 0 : 1, data_size, out_data_size, out_data);
        case kAudioDevicePropertyLatency:
        case kAudioDevicePropertySafetyOffset:
        case kAudioDevicePropertyUsesVariableBufferFrameSizes:
        case kAudioDevicePropertyIsHidden:
            return write_u32(0, data_size, out_data_size, out_data);
        case kAudioDevicePropertyStreams:
            if (address->mScope == kAudioObjectPropertyScopeInput) {
                return write_object_ids(NULL, 0, data_size, out_data_size, out_data);
            } else {
                AudioObjectID ids[] = { RUSTY_JACK_STREAM_OBJECT_ID };
                return write_object_ids(ids, 1, data_size, out_data_size, out_data);
            }
        case kAudioObjectPropertyControlList: {
            AudioObjectID ids[] = { RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID, RUSTY_JACK_MUTE_CONTROL_OBJECT_ID };
            return write_object_ids(ids, 2, data_size, out_data_size, out_data);
        }
        case kAudioDevicePropertyNominalSampleRate:
            return write_f64(RUSTY_JACK_SAMPLE_RATE, data_size, out_data_size, out_data);
        case kAudioDevicePropertyAvailableNominalSampleRates: {
            AudioValueRange range = { .mMinimum = RUSTY_JACK_SAMPLE_RATE, .mMaximum = RUSTY_JACK_SAMPLE_RATE };
            return write_data(&range, sizeof(range), data_size, out_data_size, out_data);
        }
        case kAudioDevicePropertyPreferredChannelsForStereo: {
            UInt32 channels[] = { 1, 2 };
            return write_data(channels, sizeof(channels), data_size, out_data_size, out_data);
        }
        case kAudioDevicePropertyBufferFrameSize:
            return write_u32(RUSTY_JACK_BUFFER_FRAME_SIZE, data_size, out_data_size, out_data);
        case kAudioDevicePropertyBufferFrameSizeRange: {
            AudioValueRange range = { .mMinimum = RUSTY_JACK_BUFFER_FRAME_SIZE, .mMaximum = RUSTY_JACK_BUFFER_FRAME_SIZE };
            return write_data(&range, sizeof(range), data_size, out_data_size, out_data);
        }
        case kAudioDevicePropertyStreamConfiguration:
            return write_stream_configuration(address->mScope, data_size, out_data_size, out_data);
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus get_stream_property_data(const AudioObjectPropertyAddress *address, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            return write_u32(object_base_class(RUSTY_JACK_STREAM_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyClass:
            return write_u32(object_class(RUSTY_JACK_STREAM_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwner:
            return write_u32(object_owner(RUSTY_JACK_STREAM_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyName:
            return write_cf_string(object_name(RUSTY_JACK_STREAM_OBJECT_ID), data_size, out_data_size, out_data);
        case kAudioObjectPropertyManufacturer:
            return write_cf_string(CFSTR("the-hcma"), data_size, out_data_size, out_data);
        case kAudioStreamPropertyIsActive:
            return write_u32(1, data_size, out_data_size, out_data);
        case kAudioStreamPropertyDirection:
            return write_u32(0, data_size, out_data_size, out_data);
        case kAudioStreamPropertyTerminalType:
            return write_u32(kAudioStreamTerminalTypeSpeaker, data_size, out_data_size, out_data);
        case kAudioStreamPropertyStartingChannel:
            return write_u32(1, data_size, out_data_size, out_data);
        case kAudioStreamPropertyLatency:
            return write_u32(0, data_size, out_data_size, out_data);
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat: {
            AudioStreamBasicDescription format = stream_format();
            return write_data(&format, sizeof(format), data_size, out_data_size, out_data);
        }
        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyAvailablePhysicalFormats: {
            AudioStreamRangedDescription description = stream_ranged_description();
            return write_data(&description, sizeof(description), data_size, out_data_size, out_data);
        }
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus get_control_property_data(AudioObjectID object_id, const AudioObjectPropertyAddress *address, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            return write_u32(object_base_class(object_id), data_size, out_data_size, out_data);
        case kAudioObjectPropertyClass:
            return write_u32(object_class(object_id), data_size, out_data_size, out_data);
        case kAudioObjectPropertyOwner:
            return write_u32(object_owner(object_id), data_size, out_data_size, out_data);
        case kAudioObjectPropertyName:
            return write_cf_string(object_name(object_id), data_size, out_data_size, out_data);
        case kAudioObjectPropertyManufacturer:
            return write_cf_string(CFSTR("the-hcma"), data_size, out_data_size, out_data);
        case kAudioControlPropertyScope:
            return write_u32(kAudioObjectPropertyScopeOutput, data_size, out_data_size, out_data);
        case kAudioControlPropertyElement:
            return write_u32(kAudioObjectPropertyElementMain, data_size, out_data_size, out_data);
        case kAudioLevelControlPropertyScalarValue:
            return write_f32(g_volume_scalar, data_size, out_data_size, out_data);
        case kAudioLevelControlPropertyDecibelValue:
            return write_f32(scalar_to_db(g_volume_scalar), data_size, out_data_size, out_data);
        case kAudioLevelControlPropertyDecibelRange: {
            AudioValueRange range = { .mMinimum = -96.0, .mMaximum = 0.0 };
            return write_data(&range, sizeof(range), data_size, out_data_size, out_data);
        }
        case kAudioLevelControlPropertyConvertScalarToDecibels: {
            if (out_data == NULL || data_size < sizeof(Float32)) {
                return kAudioHardwareBadPropertySizeError;
            }
            Float32 value = *((Float32 *)out_data);
            return write_f32(scalar_to_db(value), data_size, out_data_size, out_data);
        }
        case kAudioLevelControlPropertyConvertDecibelsToScalar: {
            if (out_data == NULL || data_size < sizeof(Float32)) {
                return kAudioHardwareBadPropertySizeError;
            }
            Float32 value = *((Float32 *)out_data);
            return write_f32(db_to_scalar(value), data_size, out_data_size, out_data);
        }
        case kAudioBooleanControlPropertyValue:
            return write_u32(g_muted, data_size, out_data_size, out_data);
        default:
            return kAudioHardwareUnknownPropertyError;
    }
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
    return kAudioHardwareUnsupportedOperationError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_destroy_device(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id) {
    (void)driver;
    (void)device_object_id;
    return kAudioHardwareUnsupportedOperationError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_add_device_client(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, const AudioServerPlugInClientInfo *client_info) {
    (void)driver;
    (void)device_object_id;
    (void)client_info;
    return device_object_id == RUSTY_JACK_DEVICE_OBJECT_ID ? noErr : kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_remove_device_client(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, const AudioServerPlugInClientInfo *client_info) {
    (void)driver;
    (void)device_object_id;
    (void)client_info;
    return device_object_id == RUSTY_JACK_DEVICE_OBJECT_ID ? noErr : kAudioHardwareBadDeviceError;
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

static Boolean STDMETHODCALLTYPE rusty_jack_has_property(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address) {
    (void)driver;
    (void)client_process_id;
    if (address == NULL || !object_exists(object_id)) {
        return false;
    }
    switch (object_id) {
        case kAudioObjectPlugInObject:
            return plugin_has_property(address->mSelector);
        case RUSTY_JACK_DEVICE_OBJECT_ID:
            return device_has_property(address->mSelector);
        case RUSTY_JACK_STREAM_OBJECT_ID:
            return stream_has_property(address->mSelector);
        case RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID:
        case RUSTY_JACK_MUTE_CONTROL_OBJECT_ID:
            return control_has_property(object_id, address->mSelector);
        default:
            return false;
    }
}

static OSStatus STDMETHODCALLTYPE rusty_jack_is_property_settable(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, Boolean *is_settable) {
    if (is_settable == NULL) {
        return kAudioHardwareBadPropertySizeError;
    }
    if (!rusty_jack_has_property(driver, object_id, client_process_id, address)) {
        return kAudioHardwareUnknownPropertyError;
    }

    *is_settable =
        (object_id == RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID &&
         (address->mSelector == kAudioLevelControlPropertyScalarValue ||
          address->mSelector == kAudioLevelControlPropertyDecibelValue)) ||
        (object_id == RUSTY_JACK_MUTE_CONTROL_OBJECT_ID &&
         address->mSelector == kAudioBooleanControlPropertyValue);
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
    return data_size_for_property(object_id, address, data_size);
}

static OSStatus STDMETHODCALLTYPE rusty_jack_get_property_data(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 data_size, UInt32 *out_data_size, void *out_data) {
    if (!rusty_jack_has_property(driver, object_id, client_process_id, address)) {
        return kAudioHardwareUnknownPropertyError;
    }

    switch (object_id) {
        case kAudioObjectPlugInObject:
            return get_plugin_property_data(address, qualifier_data_size, qualifier_data, data_size, out_data_size, out_data);
        case RUSTY_JACK_DEVICE_OBJECT_ID:
            return get_device_property_data(address, data_size, out_data_size, out_data);
        case RUSTY_JACK_STREAM_OBJECT_ID:
            return get_stream_property_data(address, data_size, out_data_size, out_data);
        case RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID:
        case RUSTY_JACK_MUTE_CONTROL_OBJECT_ID:
            return get_control_property_data(object_id, address, data_size, out_data_size, out_data);
        default:
            return kAudioHardwareBadObjectError;
    }
}

static OSStatus STDMETHODCALLTYPE rusty_jack_set_property_data(AudioServerPlugInDriverRef driver, AudioObjectID object_id, pid_t client_process_id, const AudioObjectPropertyAddress *address, UInt32 qualifier_data_size, const void *qualifier_data, UInt32 data_size, const void *data) {
    (void)qualifier_data_size;
    (void)qualifier_data;
    Boolean is_settable = false;
    OSStatus status = rusty_jack_is_property_settable(driver, object_id, client_process_id, address, &is_settable);
    if (status != noErr) {
        return status;
    }
    if (!is_settable || data == NULL) {
        return kAudioHardwareUnsupportedOperationError;
    }

    if (object_id == RUSTY_JACK_VOLUME_CONTROL_OBJECT_ID) {
        if (data_size < sizeof(Float32)) {
            return kAudioHardwareBadPropertySizeError;
        }
        Float32 value = *((const Float32 *)data);
        if (address->mSelector == kAudioLevelControlPropertyDecibelValue) {
            value = db_to_scalar(value);
        }
        g_volume_scalar = clamp_scalar(value);
        return noErr;
    }

    if (object_id == RUSTY_JACK_MUTE_CONTROL_OBJECT_ID) {
        if (data_size < sizeof(UInt32)) {
            return kAudioHardwareBadPropertySizeError;
        }
        g_muted = *((const UInt32 *)data) == 0 ? 0 : 1;
        return noErr;
    }

    return kAudioHardwareUnsupportedOperationError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_start_io(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id) {
    (void)driver;
    (void)client_id;
    if (device_object_id != RUSTY_JACK_DEVICE_OBJECT_ID) {
        return kAudioHardwareBadDeviceError;
    }
    if (atomic_fetch_add_explicit(&g_io_client_count, 1, memory_order_relaxed) == 0) {
        atomic_store_explicit(&g_io_start_host_time, mach_absolute_time(), memory_order_relaxed);
    }
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_stop_io(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id) {
    (void)driver;
    (void)client_id;
    if (device_object_id != RUSTY_JACK_DEVICE_OBJECT_ID) {
        return kAudioHardwareBadDeviceError;
    }
    unsigned int current = atomic_load_explicit(&g_io_client_count, memory_order_relaxed);
    while (current > 0) {
        if (atomic_compare_exchange_weak_explicit(&g_io_client_count, &current, current - 1, memory_order_relaxed, memory_order_relaxed)) {
            return noErr;
        }
    }
    return noErr;
}

static Float64 host_ticks_to_seconds(UInt64 ticks) {
    static mach_timebase_info_data_t timebase = {0, 0};
    if (timebase.denom == 0) {
        (void)mach_timebase_info(&timebase);
    }
    return ((Float64)ticks * (Float64)timebase.numer / (Float64)timebase.denom) / 1000000000.0;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_get_zero_time_stamp(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, Float64 *sample_time, UInt64 *host_time, UInt64 *seed) {
    (void)driver;
    (void)client_id;
    if (device_object_id != RUSTY_JACK_DEVICE_OBJECT_ID) {
        return kAudioHardwareBadDeviceError;
    }
    if (sample_time == NULL || host_time == NULL || seed == NULL) {
        return kAudioHardwareBadPropertySizeError;
    }

    UInt64 now = mach_absolute_time();
    UInt64 start = atomic_load_explicit(&g_io_start_host_time, memory_order_relaxed);
    if (start == 0 || start > now) {
        start = now;
    }
    *sample_time = host_ticks_to_seconds(now - start) * RUSTY_JACK_SAMPLE_RATE;
    *host_time = now;
    *seed = 1;
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_will_do_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, Boolean *will_do, Boolean *will_do_in_place) {
    (void)driver;
    (void)client_id;
    if (device_object_id != RUSTY_JACK_DEVICE_OBJECT_ID) {
        return kAudioHardwareBadDeviceError;
    }
    if (will_do == NULL || will_do_in_place == NULL) {
        return kAudioHardwareBadPropertySizeError;
    }

    *will_do = operation_id == kAudioServerPlugInIOOperationThread ||
               operation_id == kAudioServerPlugInIOOperationCycle ||
               operation_id == kAudioServerPlugInIOOperationWriteMix;
    *will_do_in_place = true;
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_begin_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info) {
    (void)driver;
    (void)client_id;
    (void)operation_id;
    (void)io_buffer_frame_size;
    (void)io_cycle_info;
    return device_object_id == RUSTY_JACK_DEVICE_OBJECT_ID ? noErr : kAudioHardwareBadDeviceError;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_do_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, AudioObjectID stream_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info, void *main_buffer, void *secondary_buffer) {
    (void)driver;
    (void)client_id;
    (void)operation_id;
    (void)io_buffer_frame_size;
    (void)io_cycle_info;
    (void)main_buffer;
    (void)secondary_buffer;
    if (device_object_id != RUSTY_JACK_DEVICE_OBJECT_ID) {
        return kAudioHardwareBadDeviceError;
    }
    if (stream_object_id != RUSTY_JACK_STREAM_OBJECT_ID) {
        return kAudioHardwareBadStreamError;
    }
    return noErr;
}

static OSStatus STDMETHODCALLTYPE rusty_jack_end_io_operation(AudioServerPlugInDriverRef driver, AudioObjectID device_object_id, UInt32 client_id, UInt32 operation_id, UInt32 io_buffer_frame_size, const AudioServerPlugInIOCycleInfo *io_cycle_info) {
    (void)driver;
    (void)client_id;
    (void)operation_id;
    (void)io_buffer_frame_size;
    (void)io_cycle_info;
    return device_object_id == RUSTY_JACK_DEVICE_OBJECT_ID ? noErr : kAudioHardwareBadDeviceError;
}
