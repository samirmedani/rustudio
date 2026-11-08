// Capture Windows limitée à la fenêtre du banc et au rectangle demandé.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3d11.h>
#include <dwmapi.h>
#include <windows.graphics.capture.interop.h>
#include <windows.graphics.directx.direct3d11.interop.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>
#include <cstring>
#include <new>

using namespace winrt;
using namespace winrt::Windows::Graphics::Capture;
using namespace winrt::Windows::Graphics::DirectX;
using namespace winrt::Windows::Graphics::DirectX::Direct3D11;
using winrt::Windows::Graphics::SizeInt32;

struct Capture {
    com_ptr<ID3D11Device> device;
    com_ptr<ID3D11DeviceContext> context;
    com_ptr<ID3D11Texture2D> staging;
    Direct3D11CaptureFramePool pool{nullptr};
    GraphicsCaptureSession session{nullptr};
    IDirect3DDevice graphics{nullptr};
    SizeInt32 size{};
    D3D11_BOX box{};

    void open(HWND hwnd, int x, int y, unsigned w, unsigned h) {
        // Les coordonnées client sont relatives au cadre visible, hors marge invisible de redimensionnement.
        RECT bounds{};
        check_hresult(DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &bounds, sizeof(bounds)));
        if (x < bounds.left || y < bounds.top) throw hresult_invalid_argument();
        box = {static_cast<UINT>(x - bounds.left), static_cast<UINT>(y - bounds.top), 0,
               static_cast<UINT>(x - bounds.left) + w, static_cast<UINT>(y - bounds.top) + h, 1};
        check_hresult(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, D3D11_SDK_VERSION, device.put(), nullptr, context.put()));
        com_ptr<IInspectable> inspectable;
        check_hresult(CreateDirect3D11DeviceFromDXGIDevice(device.as<IDXGIDevice>().get(), inspectable.put()));
        graphics = inspectable.as<IDirect3DDevice>();
        auto interop = get_activation_factory<GraphicsCaptureItem, IGraphicsCaptureItemInterop>();
        GraphicsCaptureItem item{nullptr};
        check_hresult(interop->CreateForWindow(hwnd, guid_of<GraphicsCaptureItem>(), put_abi(item)));
        size = item.Size();
        pool = Direct3D11CaptureFramePool::CreateFreeThreaded(graphics, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size);
        session = pool.CreateCaptureSession(item);
        session.IsCursorCaptureEnabled(false);
        D3D11_TEXTURE2D_DESC texture{w, h, 1, 1, DXGI_FORMAT_B8G8R8A8_UNORM,
            {1, 0}, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ, 0};
        check_hresult(device->CreateTexture2D(&texture, nullptr, staging.put()));
        session.StartCapture();
    }

    HRESULT grab(void *destination) {
        auto frame = pool.TryGetNextFrame();
        if (!frame) return S_FALSE;
        if (auto newer = pool.TryGetNextFrame()) frame = std::move(newer);
        const auto content = frame.ContentSize();
        if (content.Width != size.Width || content.Height != size.Height) {
            frame.Close();
            size = content;
            pool.Recreate(graphics, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size);
            return S_FALSE;
        }
        if (box.right > static_cast<UINT>(size.Width) || box.bottom > static_cast<UINT>(size.Height))
            throw hresult_invalid_argument();
        com_ptr<ID3D11Texture2D> texture;
        auto access = frame.Surface().as<::Windows::Graphics::DirectX::Direct3D11::IDirect3DDxgiInterfaceAccess>();
        check_hresult(access->GetInterface(guid_of<ID3D11Texture2D>(), texture.put_void()));
        context->CopySubresourceRegion(staging.get(), 0, 0, 0, 0, texture.get(), 0, &box);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        check_hresult(context->Map(staging.get(), 0, D3D11_MAP_READ, 0, &mapped));
        const auto *source = static_cast<const unsigned char *>(mapped.pData);
        auto *target = static_cast<unsigned char *>(destination);
        const size_t stride = (box.right - box.left) * 4;
        for (UINT y = 0; y < box.bottom - box.top; ++y)
            std::memcpy(target + y * stride, source + y * mapped.RowPitch, stride);
        context->Unmap(staging.get(), 0);
        return S_OK;
    }

    ~Capture() {
        try { if (session) session.Close(); } catch (...) {}
        try { if (pool) pool.Close(); } catch (...) {}
    }
};

extern "C" __declspec(dllexport) HRESULT capture_open(HWND hwnd, int x, int y, unsigned w, unsigned h, Capture **out) {
    try {
        // La boucle Python s’exécute toujours sur ce fil.
        static thread_local const bool apartment = [] { init_apartment(); return true; }();
        (void)apartment;
        auto capture = std::make_unique<Capture>();
        capture->open(hwnd, x, y, w, h);
        *out = capture.release();
        return S_OK;
    } catch (...) {
        return to_hresult();
    }
}

extern "C" __declspec(dllexport) HRESULT capture_grab(Capture *capture, void *pixels) {
    try { return capture->grab(pixels); } catch (...) { return to_hresult(); }
}

extern "C" __declspec(dllexport) void capture_close(Capture *capture) {
    delete capture;
}
