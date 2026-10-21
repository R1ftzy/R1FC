#include <windows.h>
#include <shobjidl.h>
#include <shlwapi.h>
#include <wrl/client.h>

#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cwctype>
#include <iterator>
#include <new>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {

const GUID kClsid = {
    0x6c1b5e3a,
    0x9d2f,
    0x4a87,
    {0xb3, 0xe1, 0x7f, 0x4c, 0x2d, 0x8a, 0x9b, 0x60},
};

HINSTANCE g_instance = nullptr;

enum class Target {
    Root,
    Png,
    Jpg,
    Webp,
    Bmp,
    Tiff,
    Gif,
    Mp4,
    Webm,
};

struct TargetInfo {
    Target target;
    const wchar_t* id;
    const wchar_t* title;
    const wchar_t* extension;
};

const TargetInfo kTargets[] = {
    {Target::Png, L"png", L"Convert to PNG", L"png"},
    {Target::Jpg, L"jpg", L"Convert to JPG", L"jpg"},
    {Target::Webp, L"webp", L"Convert to WebP", L"webp"},
    {Target::Bmp, L"bmp", L"Convert to BMP", L"bmp"},
    {Target::Tiff, L"tiff", L"Convert to TIFF", L"tiff"},
    {Target::Gif, L"gif", L"Convert to GIF", L"gif"},
    {Target::Mp4, L"mp4", L"Convert to MP4", L"mp4"},
    {Target::Webm, L"webm", L"Convert to WebM", L"webm"},
};

const TargetInfo* FindTarget(Target target) {
    for (const auto& info : kTargets) {
        if (info.target == target) {
            return &info;
        }
    }
    return nullptr;
}

std::wstring Lower(std::wstring value) {
    std::transform(value.begin(), value.end(), value.begin(), [](wchar_t character) {
        return static_cast<wchar_t>(std::towlower(character));
    });
    return value;
}

std::wstring ExtensionOf(const std::wstring& path) {
    const size_t slash = path.find_last_of(L"\\/");
    const size_t dot = path.find_last_of(L'.');
    if (dot == std::wstring::npos ||
        (slash != std::wstring::npos && dot < slash)) {
        return L"";
    }
    return Lower(path.substr(dot));
}

std::wstring NormalizeExtension(const std::wstring& extension) {
    if (extension == L".jpeg") {
        return L".jpg";
    }
    if (extension == L".tif") {
        return L".tiff";
    }
    return extension;
}

bool IsImageExtension(const std::wstring& extension) {
    static const wchar_t* kImageExtensions[] = {
        L".png", L".jpg", L".webp", L".bmp", L".tiff",
        L".gif", L".ppm", L".pgm", L".pbm", L".pnm", L".ico",
    };
    for (const wchar_t* candidate : kImageExtensions) {
        if (extension == candidate) {
            return true;
        }
    }
    return false;
}

bool IsVideoExtension(const std::wstring& extension) {
    static const wchar_t* kVideoExtensions[] = {
        L".mp4", L".m4v", L".mov", L".mkv", L".webm", L".avi",
    };
    for (const wchar_t* candidate : kVideoExtensions) {
        if (extension == candidate) {
            return true;
        }
    }
    return false;
}

std::wstring TargetExtension(Target target) {
    const TargetInfo* info = FindTarget(target);
    if (!info) {
        return L"";
    }
    return std::wstring(L".") + info->extension;
}

bool TargetSupportedFor(Target target, const std::wstring& extension) {
    if (extension.empty()) {
        return false;
    }
    if (TargetExtension(target) == extension) {
        return false;
    }

    const bool image = IsImageExtension(extension);
    const bool video = IsVideoExtension(extension);

    switch (target) {
        case Target::Png:
        case Target::Jpg:
        case Target::Bmp:
        case Target::Tiff:
            return image;
        case Target::Webp:
        case Target::Gif:
            return image || video;
        case Target::Mp4:
        case Target::Webm:
            return video || extension == L".gif";
        default:
            return false;
    }
}

struct Selection {
    std::vector<std::wstring> paths;
    std::vector<std::wstring> extensions;
};

bool AnalyzeSelection(IShellItemArray* items, Selection& selection) {
    if (!items) {
        return false;
    }

    DWORD count = 0;
    if (FAILED(items->GetCount(&count))) {
        return false;
    }

    for (DWORD index = 0; index < count; ++index) {
        ComPtr<IShellItem> item;
        if (FAILED(items->GetItemAt(index, item.GetAddressOf())) || !item) {
            continue;
        }

        SFGAOF attributes = 0;
        if (SUCCEEDED(item->GetAttributes(SFGAO_FOLDER, &attributes)) &&
            (attributes & SFGAO_FOLDER) != 0) {
            continue;
        }

        PWSTR raw_path = nullptr;
        if (FAILED(item->GetDisplayName(SIGDN_FILESYSPATH, &raw_path)) ||
            !raw_path) {
            continue;
        }

        std::wstring path(raw_path);
        CoTaskMemFree(raw_path);

        std::wstring extension = NormalizeExtension(ExtensionOf(path));
        if (!IsImageExtension(extension) && !IsVideoExtension(extension)) {
            continue;
        }

        selection.paths.push_back(std::move(path));
        selection.extensions.push_back(std::move(extension));
    }

    return !selection.paths.empty();
}

bool TargetApplicable(const Selection& selection, Target target) {
    bool any = false;
    const std::wstring target_extension = TargetExtension(target);

    for (size_t index = 0; index < selection.paths.size(); ++index) {
        const std::wstring& extension = selection.extensions[index];
        if (extension == target_extension) {
            continue;
        }
        if (!TargetSupportedFor(target, extension)) {
            return false;
        }
        any = true;
    }

    return any;
}

bool AnyTargetApplicable(const Selection& selection) {
    for (const auto& info : kTargets) {
        if (TargetApplicable(selection, info.target)) {
            return true;
        }
    }
    return false;
}

const wchar_t* TitleFor(Target target) {
    if (target == Target::Root) {
        return L"Convert with R1fc";
    }
    const TargetInfo* info = FindTarget(target);
    return info ? info->title : L"Convert";
}

std::wstring ModuleDirectory() {
    wchar_t buffer[32768];
    const DWORD length = GetModuleFileNameW(
        g_instance,
        buffer,
        static_cast<DWORD>(std::size(buffer)));

    if (length == 0) {
        return L"";
    }

    std::wstring path(buffer, length);
    const size_t slash = path.find_last_of(L"\\/");
    if (slash != std::wstring::npos) {
        path.resize(slash);
    }
    return path;
}

bool WriteResponseFile(
    const std::vector<std::wstring>& files,
    std::wstring& response_path) {
    wchar_t temp_directory[32768];
    const DWORD temp_length = GetTempPathW(
        static_cast<DWORD>(std::size(temp_directory)),
        temp_directory);
    if (temp_length == 0 || temp_length >= std::size(temp_directory)) {
        return false;
    }

    wchar_t temp_file[32768];
    if (GetTempFileNameW(temp_directory, L"r1f", 0, temp_file) == 0) {
        return false;
    }

    HANDLE handle = CreateFileW(
        temp_file,
        GENERIC_WRITE,
        0,
        nullptr,
        CREATE_ALWAYS,
        FILE_ATTRIBUTE_TEMPORARY,
        nullptr);
    if (handle == INVALID_HANDLE_VALUE) {
        DeleteFileW(temp_file);
        return false;
    }

    bool ok = true;
    for (const auto& file : files) {
        const DWORD byte_count = static_cast<DWORD>(
            file.size() * sizeof(wchar_t));
        DWORD written = 0;

        if (!WriteFile(handle, file.data(), byte_count, &written, nullptr) ||
            written != byte_count) {
            ok = false;
            break;
        }

        const wchar_t newline[2] = {L'\r', L'\n'};
        if (!WriteFile(handle, newline, sizeof(newline), &written, nullptr) ||
            written != sizeof(newline)) {
            ok = false;
            break;
        }
    }

    CloseHandle(handle);

    if (!ok) {
        DeleteFileW(temp_file);
        return false;
    }

    response_path = temp_file;
    return true;
}

HRESULT SpawnWorker(Target target, const Selection& selection) {
    const std::wstring target_extension = TargetExtension(target);
    std::vector<std::wstring> files;

    for (size_t index = 0; index < selection.paths.size(); ++index) {
        if (selection.extensions[index] == target_extension) {
            continue;
        }
        files.push_back(selection.paths[index]);
    }

    if (files.empty()) {
        return S_OK;
    }

    std::wstring response_path;
    if (!WriteResponseFile(files, response_path)) {
        MessageBoxW(
            nullptr,
            L"R1fc Convert could not create a temporary file list.",
            L"R1fc Convert",
            MB_OK | MB_ICONERROR);
        return E_FAIL;
    }

    const std::wstring worker = ModuleDirectory() + L"\\r1fc-convert.exe";
    if (GetFileAttributesW(worker.c_str()) == INVALID_FILE_ATTRIBUTES) {
        DeleteFileW(response_path.c_str());
        MessageBoxW(
            nullptr,
            L"R1fc Convert could not find r1fc-convert.exe next to the shell extension.",
            L"R1fc Convert",
            MB_OK | MB_ICONERROR);
        return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
    }

    const TargetInfo* info = FindTarget(target);
    if (!info) {
        DeleteFileW(response_path.c_str());
        return E_INVALIDARG;
    }

    std::wstring command =
        L"\"" + worker + L"\" --target " + info->id +
        L" --response-file \"" + response_path + L"\"";

    STARTUPINFOW startup_info{};
    startup_info.cb = sizeof(startup_info);
    PROCESS_INFORMATION process_info{};

    const BOOL started = CreateProcessW(
        worker.c_str(),
        command.data(),
        nullptr,
        nullptr,
        FALSE,
        CREATE_NO_WINDOW,
        nullptr,
        nullptr,
        &startup_info,
        &process_info);

    if (!started) {
        const DWORD error = GetLastError();
        DeleteFileW(response_path.c_str());

        wchar_t message[512];
        swprintf_s(
            message,
            L"R1fc Convert could not start the conversion worker.\n\n"
            L"Windows error: %lu",
            error);
        MessageBoxW(
            nullptr,
            message,
            L"R1fc Convert",
            MB_OK | MB_ICONERROR);
        return HRESULT_FROM_WIN32(error);
    }

    CloseHandle(process_info.hThread);
    CloseHandle(process_info.hProcess);
    return S_OK;
}

class ConvertCommand final : public IExplorerCommand {
public:
    explicit ConvertCommand(Target target) : target_(target) {}

    IFACEMETHODIMP QueryInterface(REFIID riid, void** result) override {
        if (!result) {
            return E_POINTER;
        }

        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_IExplorerCommand)) {
            *result = static_cast<IExplorerCommand*>(this);
            AddRef();
            return S_OK;
        }

        *result = nullptr;
        return E_NOINTERFACE;
    }

    IFACEMETHODIMP_(ULONG) AddRef() override {
        return ++references_;
    }

    IFACEMETHODIMP_(ULONG) Release() override {
        const ULONG remaining = --references_;
        if (remaining == 0) {
            delete this;
        }
        return remaining;
    }

    IFACEMETHODIMP GetTitle(
        IShellItemArray*,
        PWSTR* title) override {
        if (!title) {
            return E_POINTER;
        }
        *title = nullptr;
        return SHStrDupW(TitleFor(target_), title);
    }

    IFACEMETHODIMP GetIcon(
        IShellItemArray*,
        PWSTR* icon) override {
        if (!icon) {
            return E_POINTER;
        }
        *icon = nullptr;

        if (target_ != Target::Root) {
            return E_NOTIMPL;
        }

        const std::wstring path = ModuleDirectory() + L"\\Assets\\r1fc.ico";
        return SHStrDupW(path.c_str(), icon);
    }

    IFACEMETHODIMP GetToolTip(
        IShellItemArray*,
        PWSTR* tooltip) override {
        if (!tooltip) {
            return E_POINTER;
        }
        *tooltip = nullptr;

        if (target_ != Target::Root) {
            return E_NOTIMPL;
        }

        return SHStrDupW(
            L"Convert images and videos with ffmpeg",
            tooltip);
    }

    IFACEMETHODIMP GetCanonicalName(GUID* canonical_name) override {
        if (!canonical_name) {
            return E_POINTER;
        }
        *canonical_name = GUID_NULL;
        return S_OK;
    }

    IFACEMETHODIMP GetState(
        IShellItemArray* items,
        BOOL,
        EXPCMDSTATE* state) override {
        if (!state) {
            return E_POINTER;
        }

        try {
            *state = ECS_HIDDEN;

            Selection selection;
            if (!AnalyzeSelection(items, selection)) {
                return S_OK;
            }

            const bool applicable =
                target_ == Target::Root
                    ? AnyTargetApplicable(selection)
                    : TargetApplicable(selection, target_);

            *state = applicable ? ECS_ENABLED : ECS_HIDDEN;
            return S_OK;
        } catch (...) {
            *state = ECS_HIDDEN;
            return S_OK;
        }
    }

    IFACEMETHODIMP GetFlags(EXPCMDFLAGS* flags) override {
        if (!flags) {
            return E_POINTER;
        }
        *flags = target_ == Target::Root ? ECF_HASSUBCOMMANDS : ECF_DEFAULT;
        return S_OK;
    }

    IFACEMETHODIMP EnumSubCommands(
        IEnumExplorerCommand** commands) override;

    IFACEMETHODIMP Invoke(
        IShellItemArray* items,
        IBindCtx*) override {
        if (target_ == Target::Root) {
            return S_OK;
        }

        try {
            Selection selection;
            if (!AnalyzeSelection(items, selection)) {
                return S_OK;
            }
            if (!TargetApplicable(selection, target_)) {
                return S_OK;
            }
            return SpawnWorker(target_, selection);
        } catch (...) {
            MessageBoxW(
                nullptr,
                L"R1fc Convert encountered an unexpected error.",
                L"R1fc Convert",
                MB_OK | MB_ICONERROR);
            return E_FAIL;
        }
    }

private:
    ~ConvertCommand() = default;

    std::atomic<ULONG> references_{1};
    Target target_;
};

class EnumCommands final : public IEnumExplorerCommand {
public:
    EnumCommands() {
        for (const auto& info : kTargets) {
            auto* command = new (std::nothrow) ConvertCommand(info.target);
            commands_.push_back(command);
        }
    }

    IFACEMETHODIMP QueryInterface(REFIID riid, void** result) override {
        if (!result) {
            return E_POINTER;
        }

        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_IEnumExplorerCommand)) {
            *result = static_cast<IEnumExplorerCommand*>(this);
            AddRef();
            return S_OK;
        }

        *result = nullptr;
        return E_NOINTERFACE;
    }

    IFACEMETHODIMP_(ULONG) AddRef() override {
        return ++references_;
    }

    IFACEMETHODIMP_(ULONG) Release() override {
        const ULONG remaining = --references_;
        if (remaining == 0) {
            delete this;
        }
        return remaining;
    }

    IFACEMETHODIMP Next(
        ULONG count,
        IExplorerCommand** items,
        ULONG* fetched) override {
        if (!items) {
            return E_POINTER;
        }
        if (fetched) {
            *fetched = 0;
        }

        ULONG written = 0;
        while (written < count && index_ < commands_.size()) {
            IExplorerCommand* command = commands_[index_++];
            if (!command) {
                ++written;
                continue;
            }
            command->AddRef();
            items[written++] = command;
        }

        if (fetched) {
            *fetched = written;
        }
        return written == count ? S_OK : S_FALSE;
    }

    IFACEMETHODIMP Skip(ULONG count) override {
        index_ += count;
        if (index_ > commands_.size()) {
            index_ = commands_.size();
        }
        return index_ < commands_.size() ? S_OK : S_FALSE;
    }

    IFACEMETHODIMP Reset() override {
        index_ = 0;
        return S_OK;
    }

    IFACEMETHODIMP Clone(IEnumExplorerCommand** clone) override {
        if (!clone) {
            return E_POINTER;
        }
        *clone = nullptr;

        auto* result = new (std::nothrow) EnumCommands();
        if (!result) {
            return E_OUTOFMEMORY;
        }

        result->index_ = index_;
        *clone = result;
        return S_OK;
    }

private:
    ~EnumCommands() {
        for (auto* command : commands_) {
            if (command) {
                command->Release();
            }
        }
    }

    std::atomic<ULONG> references_{1};
    std::vector<ConvertCommand*> commands_;
    size_t index_ = 0;
};

IFACEMETHODIMP ConvertCommand::EnumSubCommands(
    IEnumExplorerCommand** commands) {
    if (!commands) {
        return E_POINTER;
    }
    *commands = nullptr;

    if (target_ != Target::Root) {
        return E_NOTIMPL;
    }

    try {
        auto* enumerator = new (std::nothrow) EnumCommands();
        if (!enumerator) {
            return E_OUTOFMEMORY;
        }
        *commands = enumerator;
        return S_OK;
    } catch (...) {
        return E_FAIL;
    }
}

class ClassFactory final : public IClassFactory {
public:
    IFACEMETHODIMP QueryInterface(REFIID riid, void** result) override {
        if (!result) {
            return E_POINTER;
        }

        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_IClassFactory)) {
            *result = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }

        *result = nullptr;
        return E_NOINTERFACE;
    }

    IFACEMETHODIMP_(ULONG) AddRef() override {
        return ++references_;
    }

    IFACEMETHODIMP_(ULONG) Release() override {
        const ULONG remaining = --references_;
        if (remaining == 0) {
            delete this;
        }
        return remaining;
    }

    IFACEMETHODIMP CreateInstance(
        IUnknown* outer,
        REFIID riid,
        void** result) override {
        if (!result) {
            return E_POINTER;
        }
        *result = nullptr;

        if (outer) {
            return CLASS_E_NOAGGREGATION;
        }

        auto* command = new (std::nothrow) ConvertCommand(Target::Root);
        if (!command) {
            return E_OUTOFMEMORY;
        }

        const HRESULT hr = command->QueryInterface(riid, result);
        command->Release();
        return hr;
    }

    IFACEMETHODIMP LockServer(BOOL) override {
        return S_OK;
    }

private:
    ~ClassFactory() = default;

    std::atomic<ULONG> references_{1};
};

}  // namespace

extern "C" HRESULT WINAPI DllGetClassObject(
    REFCLSID clsid,
    REFIID riid,
    void** result) {
    if (!result) {
        return E_POINTER;
    }
    *result = nullptr;

    if (!IsEqualCLSID(clsid, kClsid)) {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    auto* factory = new (std::nothrow) ClassFactory();
    if (!factory) {
        return E_OUTOFMEMORY;
    }

    const HRESULT hr = factory->QueryInterface(riid, result);
    factory->Release();
    return hr;
}

extern "C" HRESULT WINAPI DllCanUnloadNow() {
    return S_FALSE;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        g_instance = instance;
        DisableThreadLibraryCalls(instance);
    }
    return TRUE;
}
