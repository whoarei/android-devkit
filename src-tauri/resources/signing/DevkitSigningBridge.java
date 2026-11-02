// Application-owned bridge. Rebuild with scripts/build-signing-bridge.cjs.
// Passwords arrive only on stdin. Failures intentionally omit exception text.
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.*;
import java.security.cert.*;
import java.security.interfaces.*;
import java.util.*;
import java.util.zip.*;
import java.nio.*;
import com.android.apksig.ApkVerifier;
import com.android.apksig.ApkSigner;

public final class DevkitSigningBridge {
    static final DataInputStream INPUT = new DataInputStream(System.in);
    static char[] password() throws Exception {
        int length = INPUT.readInt();
        if (length < 1 || length > 4096) throw new IllegalArgumentException();
        byte[] bytes = INPUT.readNBytes(length);
        if (bytes.length != length) throw new EOFException();
        char[] chars = new String(bytes, StandardCharsets.UTF_8).toCharArray();
        Arrays.fill(bytes, (byte) 0);
        return chars;
    }
    static String hex(byte[] bytes) {
        return HexFormat.of().formatHex(bytes);
    }
    static char[] legacyPassword(char[] password) {
        // keytool's non-console Password.readPassword reads stdin byte by byte.
        // Match its UTF-8-byte representation without changing the user's input.
        byte[] bytes = new String(password).getBytes(StandardCharsets.UTF_8);
        char[] converted = new String(bytes, StandardCharsets.ISO_8859_1).toCharArray();
        Arrays.fill(bytes, (byte)0);
        return converted;
    }
    static KeyStore load(String file, char[] password) throws Exception {
        try { return KeyStore.getInstance(new File(file), password); }
        catch (IOException original) {
            char[] legacy = legacyPassword(password);
            try { return KeyStore.getInstance(new File(file), legacy); }
            finally { Arrays.fill(legacy, '\0'); }
        }
    }
    static Key privateKey(KeyStore store, String alias, char[] password) throws Exception {
        try { return store.getKey(alias, password); }
        catch (UnrecoverableKeyException original) {
            char[] legacy = legacyPassword(password);
            try { return store.getKey(alias, legacy); }
            finally { Arrays.fill(legacy, '\0'); }
        }
    }
    static Map<String,Object> certificate(X509Certificate cert) throws Exception {
        PublicKey key = cert.getPublicKey();
        int bits = key instanceof RSAKey ? ((RSAKey)key).getModulus().bitLength()
            : key instanceof ECKey ? ((ECKey)key).getParams().getOrder().bitLength() : 0;
        Map<String,Object> result = new LinkedHashMap<>();
        result.put("subject", cert.getSubjectX500Principal().getName());
        result.put("issuer", cert.getIssuerX500Principal().getName());
        result.put("notBefore", cert.getNotBefore().toInstant().getEpochSecond());
        result.put("notAfter", cert.getNotAfter().toInstant().getEpochSecond());
        result.put("algorithm", key.getAlgorithm()); result.put("bits", bits);
        result.put("sha1", hex(MessageDigest.getInstance("SHA-1").digest(cert.getEncoded())));
        result.put("sha256", hex(MessageDigest.getInstance("SHA-256").digest(cert.getEncoded())));
        result.put("der", Base64.getEncoder().encodeToString(cert.getEncoded()));
        return result;
    }
    static Object inspect(String file, String alias, boolean checkKey, boolean legacyAllowed) throws Exception {
        char[] storePassword = password();
        char[] keyPassword = null;
        try {
            KeyStore store = legacyAllowed ? load(file, storePassword) : KeyStore.getInstance(new File(file), storePassword);
            List<Object> entries = new ArrayList<>();
            List<String> aliases = Collections.list(store.aliases());
            if (aliases.size() > 256) throw new IllegalArgumentException();
            Collections.sort(aliases);
            for (String name : aliases) {
                java.security.cert.Certificate cert = store.getCertificate(name);
                if (!(cert instanceof X509Certificate)) continue;
                Map<String,Object> entry = new LinkedHashMap<>();
                entry.put("alias", name);
                entry.put("privateKey", store.entryInstanceOf(name, KeyStore.PrivateKeyEntry.class));
                entry.put("certificate", certificate((X509Certificate)cert));
                entries.add(entry);
            }
            if (checkKey) {
                keyPassword = password();
                Key key = legacyAllowed ? privateKey(store, alias, keyPassword) : store.getKey(alias, keyPassword);
                if (!(key instanceof PrivateKey)) throw new KeyStoreException();
                X509Certificate cert = (X509Certificate)store.getCertificate(alias);
                cert.checkValidity();
                String alg = key.getAlgorithm();
                String signature = alg.equals("RSA") ? "SHA256withRSA" : alg.equals("EC") ? "SHA256withECDSA" : "";
                if (signature.isEmpty()) throw new InvalidKeyException();
                byte[] challenge = new byte[32]; new SecureRandom().nextBytes(challenge);
                Signature signer = Signature.getInstance(signature);
                signer.initSign((PrivateKey)key); signer.update(challenge);
                byte[] signed = signer.sign();
                signer.initVerify(cert); signer.update(challenge);
                if (!signer.verify(signed)) throw new InvalidKeyException();
            }
            return Map.of("format", store.getType().toUpperCase(Locale.ROOT), "entries", entries);
        } finally {
            Arrays.fill(storePassword, '\0');
            if (keyPassword != null) Arrays.fill(keyPassword, '\0');
        }
    }
    static Object verify(String file) throws Exception {
        ApkVerifier.Result verification = new ApkVerifier.Builder(new File(file)).build().verify();
        List<Object> signers = new ArrayList<>();
        for (X509Certificate cert : verification.getSignerCertificates()) signers.add(certificate(cert));
        List<String> schemes = new ArrayList<>();
        if (verification.isVerifiedUsingV1Scheme()) schemes.add("v1");
        if (verification.isVerifiedUsingV2Scheme()) schemes.add("v2");
        if (verification.isVerifiedUsingV3Scheme()) schemes.add("v3");
        if (verification.isVerifiedUsingV31Scheme()) schemes.add("v3.1");
        List<String> errors = new ArrayList<>();
        for (ApkVerifier.IssueWithParams issue : verification.getAllErrors()) errors.add(issue.getIssue().name());
        List<String> warnings = new ArrayList<>();
        for (ApkVerifier.IssueWithParams issue : verification.getWarnings()) warnings.add(issue.getIssue().name());
        for (ApkVerifier.Result.V1SchemeSignerInfo signer : verification.getV1SchemeSigners())
            for (ApkVerifier.IssueWithParams issue : signer.getWarnings()) warnings.add(issue.getIssue().name());
        for (ApkVerifier.Result.V2SchemeSignerInfo signer : verification.getV2SchemeSigners())
            for (ApkVerifier.IssueWithParams issue : signer.getWarnings()) warnings.add(issue.getIssue().name());
        for (ApkVerifier.Result.V3SchemeSignerInfo signer : verification.getV3SchemeSigners())
            for (ApkVerifier.IssueWithParams issue : signer.getWarnings()) warnings.add(issue.getIssue().name());
        boolean lineage = verification.getSigningCertificateLineage() != null;
        for (ApkVerifier.Result.V3SchemeSignerInfo signer : verification.getV3SchemeSigners()) {
            lineage |= signer.getSigningCertificateLineage() != null;
        }
        for (ApkVerifier.Result.V3SchemeSignerInfo signer : verification.getV31SchemeSigners()) {
            lineage |= signer.getSigningCertificateLineage() != null;
        }
        Map<String,Object> result = new LinkedHashMap<>();
        result.put("verified", verification.isVerified()); result.put("signers", signers);
        result.put("schemes", schemes); result.put("errors", errors); result.put("warnings", warnings);
        result.put("lineage", lineage);
        boolean unsigned = !verification.isVerified() && !errors.isEmpty()
            && errors.stream().allMatch(error -> error.equals("JAR_SIG_NO_MANIFEST") || error.equals("JAR_SIG_NO_SIGNATURES"))
            && !hasSignatureMaterial(file);
        result.put("unsigned", unsigned);
        return result;
    }
    static Object sign(String[] args) throws Exception {
        char[] storePassword = password(); char[] keyPassword = password();
        try {
            KeyStore store = load(args[1], storePassword);
            Key key = privateKey(store, args[2], keyPassword);
            if (!(key instanceof PrivateKey)) throw new InvalidKeyException();
            List<X509Certificate> chain = new ArrayList<>();
            for (java.security.cert.Certificate cert : store.getCertificateChain(args[2])) chain.add((X509Certificate)cert);
            chain.get(0).checkValidity();
            if (Files.exists(Path.of(args[4]))) throw new IOException();
            ApkSigner.SignerConfig config = new ApkSigner.SignerConfig.Builder("DEVKIT", (PrivateKey)key, chain).build();
            new ApkSigner.Builder(List.of(config)).setInputApk(new File(args[3])).setOutputApk(new File(args[4]))
                .setOtherSignersSignaturesPreserved(false).setV4SigningEnabled(false)
                .setAlignmentPreserved(true).setLibraryPageAlignmentBytes(16384).build().sign();
            return Map.of("signed", true);
        } finally { Arrays.fill(storePassword, '\0'); Arrays.fill(keyPassword, '\0'); }
    }
    static Object normalize(String[] args) throws Exception {
        char[] storePassword = password(); char[] keyPassword = password();
        try {
            KeyStore store = load(args[1], storePassword);
            if (!store.getType().equalsIgnoreCase("JKS")) throw new KeyStoreException();
            Key key = privateKey(store, args[2], keyPassword);
            store.setKeyEntry(args[2], key, keyPassword, store.getCertificateChain(args[2]));
            try (java.nio.channels.FileChannel channel = java.nio.channels.FileChannel.open(Path.of(args[3]), StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)) {
                store.store(java.nio.channels.Channels.newOutputStream(channel), storePassword);
                channel.force(true);
            }
            return Map.of("normalized", true);
        } finally { Arrays.fill(storePassword, '\0'); Arrays.fill(keyPassword, '\0'); }
    }
    static boolean hasSignatureMaterial(String file) throws Exception {
        try (ZipFile zip = new ZipFile(file)) {
            Enumeration<? extends ZipEntry> entries = zip.entries();
            int count = 0;
            while (entries.hasMoreElements()) {
                if (++count > 100000) throw new IllegalArgumentException();
                String name = entries.nextElement().getName().toUpperCase(Locale.ROOT);
                if (name.startsWith("META-INF/") && (name.endsWith(".SF") || name.endsWith(".RSA") || name.endsWith(".DSA") || name.endsWith(".EC"))) return true;
            }
        }
        try (RandomAccessFile input = new RandomAccessFile(file, "r")) {
            int length = (int)Math.min(input.length(), 65557);
            byte[] tail = new byte[length]; input.seek(input.length() - length); input.readFully(tail);
            ByteBuffer bytes = ByteBuffer.wrap(tail).order(ByteOrder.LITTLE_ENDIAN);
            for (int i = length - 22; i >= 0; i--) {
                if (bytes.getInt(i) == 0x06054b50 && i + 22 + Short.toUnsignedInt(bytes.getShort(i + 20)) == length) {
                    long central = Integer.toUnsignedLong(bytes.getInt(i + 16));
                    if (central < 16) return false;
                    byte[] magic = new byte[16]; input.seek(central - 16); input.readFully(magic);
                    return Arrays.equals(magic, "APK Sig Block 42".getBytes(StandardCharsets.US_ASCII));
                }
            }
            throw new IllegalArgumentException();
        }
    }
    static String json(Object value) {
        if (value == null) return "null";
        if (value instanceof String) {
            StringBuilder out = new StringBuilder("\"");
            for (char c : ((String)value).toCharArray()) {
                if (c == '"' || c == '\\') out.append('\\').append(c);
                else if (c < 32) out.append(String.format("\\u%04x", (int)c));
                else out.append(c);
            }
            return out.append('"').toString();
        }
        if (value instanceof Map) {
            List<String> entries = new ArrayList<>();
            for (Object item : ((Map<?,?>)value).entrySet()) {
                Map.Entry<?,?> entry = (Map.Entry<?,?>)item;
                entries.add(json(entry.getKey().toString()) + ":" + json(entry.getValue()));
            }
            return "{" + String.join(",", entries) + "}";
        }
        if (value instanceof Iterable) {
            List<String> entries = new ArrayList<>();
            for (Object entry : (Iterable<?>)value) entries.add(json(entry));
            return "[" + String.join(",", entries) + "]";
        }
        if (value instanceof Boolean || value instanceof Number) return value.toString();
        throw new IllegalArgumentException();
    }
    public static void main(String[] args) {
        try {
            Object result;
            switch (args[0]) {
                case "probe": result = Map.of("ready", true); break;
                case "signing-probe":
                    Class.forName("com.android.apksig.ApkSigner$Builder").getMethod("setLibraryPageAlignmentBytes", int.class);
                    result = Map.of("ready", true); break;
                case "inspect": result = inspect(args[1], "", false, true); break;
                case "keycheck": result = inspect(args[1], args[2], true, true); break;
                case "strict-keycheck": result = inspect(args[1], args[2], true, false); break;
                case "normalize": result = normalize(args); break;
                case "verify": result = verify(args[1]); break;
                case "sign": result = sign(args); break;
                case "certificate":
                    if (Files.size(Path.of(args[1])) > 1048576) throw new IllegalArgumentException();
                    try (InputStream input = Files.newInputStream(Path.of(args[1]))) {
                        result = certificate((X509Certificate)CertificateFactory.getInstance("X.509").generateCertificate(input));
                    }
                    break;
                default: throw new IllegalArgumentException();
            }
            System.out.write(json(result).getBytes(StandardCharsets.UTF_8));
            System.out.flush();
        } catch (Throwable error) {
            // A provider can include its input in exception messages. Never forward those.
            byte[] failure = json(Map.of("bridgeError", error.getClass().getSimpleName())).getBytes(StandardCharsets.UTF_8);
            System.out.write(failure, 0, failure.length);
            System.out.flush();
            System.exit(2);
        }
    }
}
