#include <windows.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <shlwapi.h>
#include <wrl/client.h>

#include <cstdio>
#include <cwchar>
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

const wchar_t* StateName(EXPCMDSTATE state) {
    switch (state) {
        case ECS_ENABLED:
            return L"ENABLED";
        case ECS_DISABLED:
            return L"DISABLED";
        case ECS_HIDDEN:
            return L"HIDDEN";
        default:
            return L"UNKNOWN";
    }
}

std::wstring TargetTitle(const std::wstring& id) {
    if (id == L"png") return L"Convert to PNG";
    if (id == L"jpg") return L"Convert to JPG";
    if (id == L"webp") return L"Convert to WebP";
    if (id == L"bmp") return L"Convert to BMP";
    if (id == L"tiff") return L"Convert to TIFF";
    if (id == L"gif") return L"Convert to GIF";
    if (id == L"mp4") return L"Convert to MP4";
    if (id == L"webm") return L"Convert to WebM";
    return L"";
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
    setvbuf(stdout, nullptr, _IONBF, 0);
    setvbuf(stderr, nullptr, _IONBF, 0);

    if (argc < 3) {
        wprintf(L"Usage: r1fc-shell-harness <dll> [--invoke <id>] [--wait <seconds>] <file>...\n");
        return 2;
    }

    const wchar_t* dll_path = argv[1];
    std::wstring invoke_id;
    int wait_seconds = 4;
    std::vector<std::wstring> files;

    for (int index = 2; index < argc; ++index) {
        const std::wstring argument = argv[index];
        if (argument == L"--invoke" && index + 1 < argc) {
            invoke_id = argv[++index];
        } else if (argument == L"--wait" && index + 1 < argc) {
            wait_seconds = _wtoi(argv[++index]);
        } else {
            files.push_back(argument);
        }
    }

    if (files.empty()) {
        wprintf(L"No input files were supplied.\n");
        return 2;
    }

    const HRESULT com_result = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(com_result)) {
        wprintf(L"CoInitializeEx failed: 0x%08lx\n", com_result);
        return 1;
    }
    HMODULE module = LoadLibraryW(dll_path);
    if (!module) {
        wprintf(L"LoadLibraryW failed: %lu\n", GetLastError());
        CoUninitialize();
        return 1;
    }
    using GetClassObjectFn = HRESULT(WINAPI*)(REFCLSID, REFIID, void**);
    auto get_class_object = reinterpret_cast<GetClassObjectFn>(
        GetProcAddress(module, "DllGetClassObject"));
    if (!get_class_object) {
        wprintf(L"DllGetClassObject was not found.\n");
        FreeLibrary(module);
        CoUninitialize();
        return 1;
    }

    ComPtr<IClassFactory> factory;
    HRESULT hr = get_class_object(
        kClsid,
        IID_IClassFactory,
        reinterpret_cast<void**>(factory.GetAddressOf()));
    if (FAILED(hr) || !factory) {
        wprintf(L"DllGetClassObject failed: 0x%08lx\n", hr);
        FreeLibrary(module);
        CoUninitialize();
        return 1;
    }
    ComPtr<IExplorerCommand> root;
    hr = factory->CreateInstance(
        nullptr,
        IID_IExplorerCommand,
        reinterpret_cast<void**>(root.GetAddressOf()));
    if (FAILED(hr) || !root) {
        wprintf(L"CreateInstance failed: 0x%08lx\n", hr);
        FreeLibrary(module);
        CoUninitialize();
        return 1;
    }
    std::vector<PIDLIST_ABSOLUTE> pidls;
    for (const auto& file : files) {
        PIDLIST_ABSOLUTE pidl = nullptr;
        hr = SHParseDisplayName(file.c_str(), nullptr, &pidl, 0, nullptr);
        if (FAILED(hr) || !pidl) {
            wprintf(L"SHParseDisplayName failed for %s: 0x%08lx\n", file.c_str(), hr);
            continue;
        }
        pidls.push_back(pidl);
    }
    ComPtr<IShellItemArray> items;
    if (!pidls.empty()) {
        std::vector<LPCITEMIDLIST> selection_pidls(pidls.begin(), pidls.end());
        hr = SHCreateShellItemArrayFromIDLists(
            static_cast<UINT>(selection_pidls.size()),
            selection_pidls.data(),
            items.GetAddressOf());
    }

    for (PIDLIST_ABSOLUTE pidl : pidls) {
        ILFree(pidl);
    }

    if (FAILED(hr) || !items) {
        wprintf(L"Could not build the shell item array: 0x%08lx\n", hr);
        FreeLibrary(module);
        CoUninitialize();
        return 1;
    }
    PWSTR title = nullptr;
    hr = root->GetTitle(items.Get(), &title);
    wprintf(L"root title: %s (0x%08lx)\n", title ? title : L"<null>", hr);
    CoTaskMemFree(title);

    EXPCMDSTATE state = ECS_HIDDEN;
    hr = root->GetState(items.Get(), TRUE, &state);
    wprintf(L"root state: %s (0x%08lx)\n", StateName(state), hr);

    EXPCMDFLAGS flags = ECF_DEFAULT;
    hr = root->GetFlags(&flags);
    wprintf(L"root flags: 0x%08lx (0x%08lx)\n", flags, hr);

    PWSTR icon = nullptr;
    hr = root->GetIcon(items.Get(), &icon);
    wprintf(L"root icon: %s (0x%08lx)\n", icon ? icon : L"<null>", hr);
    CoTaskMemFree(icon);

    ComPtr<IEnumExplorerCommand> enumerator;
    hr = root->EnumSubCommands(enumerator.GetAddressOf());
    if (FAILED(hr) || !enumerator) {
        wprintf(L"EnumSubCommands failed: 0x%08lx\n", hr);
        FreeLibrary(module);
        CoUninitialize();
        return 1;
    }

    ComPtr<IExplorerCommand> child;
    ULONG fetched = 0;
    bool invoked = false;

    while (enumerator->Next(1, child.ReleaseAndGetAddressOf(), &fetched) == S_OK) {
        PWSTR child_title = nullptr;
        child->GetTitle(items.Get(), &child_title);

        EXPCMDSTATE child_state = ECS_HIDDEN;
        child->GetState(items.Get(), TRUE, &child_state);

        wprintf(
            L"  %s: %s\n",
            child_title ? child_title : L"<null>",
            StateName(child_state));

        if (!invoke_id.empty() && child_title &&
            _wcsicmp(child_title, TargetTitle(invoke_id).c_str()) == 0) {
            const HRESULT invoke_result = child->Invoke(items.Get(), nullptr);
            wprintf(L"  invoked: 0x%08lx\n", invoke_result);
            invoked = true;
        }

        CoTaskMemFree(child_title);
    }

    if (!invoke_id.empty() && !invoked) {
        wprintf(L"Requested target was not found: %s\n", invoke_id.c_str());
    }

    if (invoked && wait_seconds > 0) {
        wprintf(L"Waiting %d seconds for the worker...\n", wait_seconds);
        Sleep(static_cast<DWORD>(wait_seconds) * 1000);
    }

    child.Reset();
    enumerator.Reset();
    root.Reset();
    factory.Reset();
    items.Reset();
    FreeLibrary(module);
    CoUninitialize();
    return 0;
}
